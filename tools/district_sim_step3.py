#!/usr/bin/env python3
"""Step 3 of the district simulators (wiki "District Simulators and Train Tracking"): the
node knows its own track. As step 1 (the district simulator for MacBook, a node on it, a
throttle driving loco 4444 out of Adorf), but the node runs with `--layout`, and a
dispatcher watches. Checks that the node names what its buses report, and that the
dispatcher shows it:

  - the node loads its own district from the layout,
  - it asks the bus's detectors for their state (the interrogate) and learns them,
  - a detector's report becomes "block <module>/<block> occupied (LocoNet sensor N)",
  - a switch command becomes "turnout timo-adorf/3 thrown (LocoNet switch 21)",
  - the dispatcher's event list shows the named blocks.

    cargo build && python3 tools/district_sim_step3.py [target/debug/hive-rail]
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from district_sim_step1 import message, read_slot, wait_for  # noqa: E402

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
SESSION = f"district-sim-step3-{os.getpid()}"
DCCEX_PORT, BUS_PORT, THROTTLE_PORT = 25600, 12350, 12351
LOCO = 4444


def main():
    logs = tempfile.mkdtemp(prefix="district-sim-")
    sim_log, node_log, dispatcher_log = (os.path.join(logs, f"{n}.log") for n in ("sim", "node", "dispatcher"))
    processes, checks = [], []

    def check(what, ok):
        checks.append((what, ok))
        print(("PASS  " if ok else "FAIL  ") + what)

    try:
        processes.append(subprocess.Popen(
            [BINARY, "--sim-district", LAYOUT, "--district", "MacBook",
             "--body", f"{LOCO}@timo-adorf/1,heading=backward,along=900,length=600,kmh=100",
             "--dccex-port", str(DCCEX_PORT), "--loconet-port", str(BUS_PORT)],
            stdout=open(sim_log, "w"), stderr=subprocess.STDOUT))
        if not wait_for(sim_log, r"join it with", 10):
            check("the district simulator starts", False)
            return
        processes.append(subprocess.Popen(
            [BINARY, "--mode", "dispatcher", "--session", SESSION, "--plain"],
            stdin=subprocess.PIPE, stdout=open(dispatcher_log, "w"), stderr=subprocess.STDOUT))
        processes.append(subprocess.Popen(
            [BINARY, "--mode", "node", "--session", SESSION, "--id", "MacBook",
             "--rocnet-address", "101", "--plain", "--layout", LAYOUT,
             "--dccex", f"127.0.0.1:{DCCEX_PORT}",
             "--loconet-connect", f"127.0.0.1:{BUS_PORT}", "--loconet-connect-slots", "node",
             "--loconet-tcp", str(THROTTLE_PORT), "--loconet-tcp-slots", "node"],
            stdin=subprocess.PIPE, stdout=open(node_log, "w"), stderr=subprocess.STDOUT))
        check("the node loads its own district from the layout",
              wait_for(node_log, r"Own track from .*: district MacBook, \d+ blocks, \d+ turnouts", 10))
        check("the node joins the simulator",
              wait_for(sim_log, r"DCC-EX client .* connected", 15) and wait_for(sim_log, r"LocoNet client .* connected", 15))
        check("the node asks the detectors (interrogate) and learns where the loco stands",
              wait_for(sim_log, r"interrogate: every detector reports", 10)
              and wait_for(node_log, r"EVENT\s+block timo-adorf/adorf_1_b occupied \(LocoNet sensor 203\)", 5))
        time.sleep(2)  # the dispatcher finds the node

        throttle = socket.create_connection(("127.0.0.1", THROTTLE_PORT), timeout=5)
        throttle.sendall(message(0x83))
        throttle.sendall(message(0xBF, LOCO >> 7, LOCO & 0x7F))
        slot = read_slot(throttle, LOCO)
        check(f"the node gives loco {LOCO} a slot", slot is not None)
        if slot is None:
            return
        throttle.sendall(message(0xBA, slot, slot))
        throttle.sendall(message(0xA1, slot, 0x10))
        throttle.sendall(message(0xA0, slot, 64))
        check("the simulator reports a block",
              wait_for(sim_log, r"block timo-2e96-west/line_2 occupied \(LocoNet sensor 202\)", 20))
        check("the node names it: block timo-2e96-west/line_2 occupied (LocoNet sensor 202)",
              wait_for(node_log, r"EVENT\s+block timo-2e96-west/line_2 occupied \(LocoNet sensor 202\)", 5))
        check("and the block it left: block timo-adorf/adorf_1_b free (LocoNet sensor 203)",
              wait_for(node_log, r"EVENT\s+block timo-adorf/adorf_1_b free \(LocoNet sensor 203\)", 5))
        throttle.sendall(message(0xB0, (21 - 1) & 0x7F, 0x10))
        throttle.sendall(message(0xB0, (21 - 1) & 0x7F, 0x00))
        check("the node names the switch command: turnout timo-adorf/3 thrown (LocoNet switch 21)",
              wait_for(node_log, r"EVENT\s+turnout timo-adorf/3 thrown \(LocoNet switch 21\)", 5))
        check("the dispatcher shows the named block in its events",
              wait_for(dispatcher_log, r"MacBook.*block timo-2e96-west/line_2 occupied", 5))
        throttle.sendall(message(0xA0, slot, 0))
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
