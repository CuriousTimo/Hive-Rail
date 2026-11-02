#!/usr/bin/env python3
"""Step 5 of the district simulators (wiki "District Simulators and Train Tracking"): the
handoff. As step 2 (district simulators for MacBook and the M5StickC on one world channel,
a real node on each, loco 4444 on MacBook's border module driven from MacBook's throttle),
but the nodes know their track (`--layout`) and an operator placed the train. Checks:

  1. MacBook's node follows the train it was given,
  2. at the border the M5StickC's node takes the train over from MacBook's picture,
  3. and takes the loco into its district: its DCC-EX drives the loco now,
  4. MacBook's node hands the train over once its border block is free,
  5. the throttle on MacBook still drives the loco: stopping it there stops it in the
     M5StickC's district,
  6. there were no ghosts and nothing ambiguous.

    cargo build && python3 tools/district_sim_step5.py [target/debug/hive-rail]
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from district_sim_step1 import message, read_slot, wait_for  # noqa: E402

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
SESSION = f"district-sim-step5-{os.getpid()}"
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
                 "--rocnet-address", str(rocnet), "--plain", "--layout", LAYOUT,
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

        subprocess.run([BINARY, "--place-train", str(LOCO), "--block", "timo-2e96-border/edge_1", "--session", SESSION],
                       stdout=subprocess.DEVNULL, check=True)
        check("MacBook's node follows the train it was given",
              wait_for(log("node-MacBook"), rf"EVENT\s+train {LOCO} placed in block timo-2e96-border/edge_1", 5))
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
        check("the M5StickC's node takes the train from MacBook's picture",
              wait_for(log("node-M5StickC"), rf"EVENT\s+train {LOCO} came in from MacBook into block m5-\S+", 5))
        check("and takes the loco into its district",
              wait_for(log("node-M5StickC"), rf"EVENT\s+Loco {LOCO} is in this district now", 5))
        check("the M5StickC's DCC-EX drives it now (the gap of step 2 is closed)",
              wait_for(log("node-M5StickC"), rf"\[DCC-EX\] -> <t {LOCO} [1-9]\d* 1>", 5))
        check("MacBook's node hands the train over once its border block is free",
              wait_for(log("node-MacBook"), rf"EVENT\s+train {LOCO} handed over to M5StickC", 15))
        macbook.sendall(message(0xA0, slot, 0))  # stop, on MacBook's throttle
        check("stopping it on MacBook's throttle reaches the M5StickC's DCC-EX",
              wait_for(log("node-M5StickC"), rf"\[DCC-EX\] -> <t {LOCO} 0 1>", 5))
        check("and the loco stops in the M5StickC's district",
              wait_for(log("sim-M5StickC"), rf"loco {LOCO} at m5-\S+ \d+ mm, decoder 0 km/h", 15))
        for district in DISTRICTS:
            text = open(log(f"node-{district}")).read()
            check(f"{district}'s node saw no ghost and nothing ambiguous after placing",
                  not re.search(r"GHOST|AMBIGUOUS", text[text.find("placed in block"):]))
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
