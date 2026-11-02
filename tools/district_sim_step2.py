#!/usr/bin/env python3
"""Step 2 of the district simulators (wiki "District Simulators and Train Tracking"): the
border. Two district simulators, MacBook and M5StickC (examples/layout/bench-m5.json), on
one world channel, each with a real node on it as on a node board. Loco 4444 stands on
MacBook's border module; a throttle on MacBook's node drives it towards the M5StickC.

Checks, from the simulators' and the nodes' logs:

  1. the simulators find each other on the world channel,
  2. the loco crosses: MacBook's simulator hands it over, the M5StickC's takes it,
  3. it runs on in the M5StickC's district at its last speed, though the M5StickC's node
     never drove it (its DCC-EX got no command for loco 4444): the gap that train
     tracking and the handoff (steps 4 and 5) close,
  4. the M5StickC's detectors report it and its node receives the reports,
  5. MacBook's border blocks go free once the train has left them,
  6. switching the M5StickC's district off stops the loco there.

    cargo build && python3 tools/district_sim_step2.py [target/debug/hive-rail]
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from district_sim_step1 import message, read_slot, wait_for  # noqa: E402

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
SESSION = f"district-sim-step2-{os.getpid()}"
LOCO = 4444
# District: (DCC-EX port, LocoNet buffer port, throttle port, RocNet address)
DISTRICTS = {"MacBook": (25600, 12350, 12351, 101), "M5StickC": (25601, 12352, 12353, 199)}


def main():
    logs = tempfile.mkdtemp(prefix="district-sim-")
    log = lambda name: os.path.join(logs, f"{name}.log")
    processes, checks = [], []

    def check(what, ok):
        checks.append((what, ok))
        print(("PASS  " if ok else "FAIL  ") + what)

    try:
        for district, (dccex, bus, _, _) in DISTRICTS.items():
            body = ["--body", f"{LOCO}@timo-2e96-border/1,heading=forward,along=300,length=300,kmh=80"] if district == "MacBook" else []
            processes.append(subprocess.Popen(
                [BINARY, "--sim-district", LAYOUT, "--district", district, "--session", SESSION,
                 "--dccex-port", str(dccex), "--loconet-port", str(bus), *body],
                stdout=open(log(f"sim-{district}"), "w"), stderr=subprocess.STDOUT))
        for district, (dccex, bus, throttle, rocnet) in DISTRICTS.items():
            if not wait_for(log(f"sim-{district}"), r"join it with", 10):
                check(f"{district}'s simulator starts", False)
                return
            processes.append(subprocess.Popen(
                [BINARY, "--mode", "node", "--session", SESSION, "--id", district,
                 "--rocnet-address", str(rocnet), "--plain",
                 "--dccex", f"127.0.0.1:{dccex}",
                 "--loconet-connect", f"127.0.0.1:{bus}", "--loconet-connect-slots", "node",
                 "--loconet-tcp", str(throttle), "--loconet-tcp-slots", "node"],
                stdin=subprocess.PIPE, stdout=open(log(f"node-{district}"), "w"), stderr=subprocess.STDOUT))
        check("the simulators find each other on the world channel",
              wait_for(log("sim-MacBook"), r"M5StickC's simulator is on the world channel", 15)
              and wait_for(log("sim-M5StickC"), r"MacBook's simulator is on the world channel", 15))
        for district in DISTRICTS:
            check(f"{district}'s node joins its simulator",
                  wait_for(log(f"sim-{district}"), r"DCC-EX client .* connected", 15)
                  and wait_for(log(f"sim-{district}"), r"LocoNet client .* connected", 15))
        time.sleep(1)

        throttles = {d: socket.create_connection(("127.0.0.1", p[2]), timeout=5) for d, p in DISTRICTS.items()}
        for district, throttle in throttles.items():
            throttle.sendall(message(0x83))  # each district's power on, from its own LocoNet
            check(f"{district}'s track has power", wait_for(log(f"sim-{district}"), r"track power ON", 5))

        macbook = throttles["MacBook"]
        macbook.sendall(message(0xBF, LOCO >> 7, LOCO & 0x7F))
        slot = read_slot(macbook, LOCO)
        check(f"MacBook's node gives loco {LOCO} a slot", slot is not None)
        if slot is None:
            return
        macbook.sendall(message(0xBA, slot, slot))
        macbook.sendall(message(0xA1, slot, 0x10))
        macbook.sendall(message(0xA0, slot, 64))

        check("MacBook's simulator hands the loco over at the border",
              wait_for(log("sim-MacBook"), rf"loco {LOCO} runs on into M5StickC", 30))
        check("the M5StickC's simulator takes it",
              wait_for(log("sim-M5StickC"), rf"loco {LOCO} came in from MacBook", 5))
        check("it runs on in the M5StickC's district at its last speed",
              wait_for(log("sim-M5StickC"), rf"loco {LOCO} at m5-\S+ \d+ mm, decoder [1-9]\d* km/h forward", 15))
        with open(log("node-M5StickC")) as node:
            drove = re.search(rf"<t {LOCO} ", node.read())
        check("the M5StickC's node never drove it (the gap steps 4 and 5 close)", drove is None)
        check("the M5StickC's detectors report it",
              wait_for(log("sim-M5StickC"), r"block m5-\S+ occupied \(LocoNet sensor \d+\)", 15))
        with open(log("sim-M5StickC")) as sim:
            sensors = set(re.findall(r"occupied \(LocoNet sensor (\d+)\)", sim.read()))
        check(f"the M5StickC's node receives the reports ({', '.join(sorted(sensors))})",
              bool(sensors) and all(wait_for(log("node-M5StickC"), rf"sensor {s} high", 5) for s in sensors))
        check("MacBook's border blocks go free once the train has left them",
              wait_for(log("sim-MacBook"), r"block timo-2e96-border/\S+ free", 15))

        throttles["M5StickC"].sendall(message(0x82))  # the M5StickC's district off
        check("switching the M5StickC's district off reaches its track",
              wait_for(log("sim-M5StickC"), r"track power OFF", 5))
        time.sleep(11)
        with open(log("sim-M5StickC")) as sim:
            last = re.findall(rf"loco {LOCO} at (\S+) (\d+) mm, decoder \d+ km/h \w+(, no track power)?", sim.read())[-1]
        time.sleep(10.5)
        with open(log("sim-M5StickC")) as sim:
            later = re.findall(rf"loco {LOCO} at (\S+) (\d+) mm", sim.read())[-1]
        check(f"the loco stands without power (at {later[0]} {later[1]} mm)",
              last[2] != "" and (last[0], last[1]) == later)
    except Exception:
        traceback.print_exc()
        check("the test itself runs without an error", False)
    finally:
        for process in reversed(processes):
            process.terminate()
        for process in processes:
            process.wait(5)
        print(f"logs in {logs}")
        sys.exit(1 if not checks or any(not ok for _, ok in checks) else 0)


if __name__ == "__main__":
    main()
