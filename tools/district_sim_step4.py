#!/usr/bin/env python3
"""Step 4 of the district simulators (wiki "District Simulators and Train Tracking"):
following trains on the node. The district simulator for MacBook has three locos in
Adorf: 4444 on platform 1, 7701 on platform 2 and 5555 on platform 3. An operator places
4444 and 7701 with `--place-train`; 5555 is never placed. A throttle drives 4444 out.

Checks, against the simulator's own log (the truth):

  - the node follows 4444 into every block the simulator reports it in, and nowhere else,
  - 7701, standing, stays where it was placed,
  - 5555's blocks are ghosts: the node never guesses whose they are,
  - nothing is ambiguous,
  - a second node (Windows) receives MacBook's train picture over the mesh.

    cargo build && python3 tools/district_sim_step4.py [target/debug/hive-rail]
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from district_sim_step1 import message, read_slot, wait_for  # noqa: E402

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
SESSION = f"district-sim-step4-{os.getpid()}"
DCCEX_PORT, BUS_PORT, THROTTLE_PORT = 25600, 12350, 12351
LOCO = 4444


def read(path):
    with open(path, errors="replace") as log:
        return log.read()


def main():
    logs = tempfile.mkdtemp(prefix="district-sim-")
    sim_log, node_log, other_log = (os.path.join(logs, f"{n}.log") for n in ("sim", "node", "windows"))
    processes, checks = [], []

    def check(what, ok):
        checks.append((what, ok))
        print(("PASS  " if ok else "FAIL  ") + what)

    try:
        bodies = []
        for loco, platform in ((LOCO, 1), (7701, 2), (5555, 3)):
            bodies += ["--body", f"{loco}@timo-adorf/{platform},heading=backward,along=900,length=500,kmh=100"]
        processes.append(subprocess.Popen(
            [BINARY, "--sim-district", LAYOUT, "--district", "MacBook", *bodies,
             "--dccex-port", str(DCCEX_PORT), "--loconet-port", str(BUS_PORT)],
            stdout=open(sim_log, "w"), stderr=subprocess.STDOUT))
        if not wait_for(sim_log, r"join it with", 10):
            check("the district simulator starts", False)
            return
        processes.append(subprocess.Popen(
            [BINARY, "--mode", "node", "--session", SESSION, "--id", "Windows",
             "--rocnet-address", "102", "--plain", "--layout", LAYOUT],
            stdin=subprocess.PIPE, stdout=open(other_log, "w"), stderr=subprocess.STDOUT))
        processes.append(subprocess.Popen(
            [BINARY, "--mode", "node", "--session", SESSION, "--id", "MacBook",
             "--rocnet-address", "101", "--plain", "--layout", LAYOUT,
             "--dccex", f"127.0.0.1:{DCCEX_PORT}",
             "--loconet-connect", f"127.0.0.1:{BUS_PORT}", "--loconet-connect-slots", "node",
             "--loconet-tcp", str(THROTTLE_PORT), "--loconet-tcp-slots", "node"],
            stdin=subprocess.PIPE, stdout=open(node_log, "w"), stderr=subprocess.STDOUT))
        check("the node learns the occupied blocks (interrogate)",
              wait_for(node_log, r"EVENT\s+block timo-adorf/adorf_3_\w+ occupied", 15))
        check("the blocks of a loco nobody placed are ghosts",
              wait_for(node_log, r"GHOST block timo-adorf/adorf_3_\w+ occupied", 5))
        for loco, block in ((LOCO, "timo-adorf/adorf_1_b"), (7701, "timo-adorf/adorf_2_b")):
            subprocess.run([BINARY, "--place-train", str(loco), "--block", block, "--session", SESSION],
                           stdout=subprocess.DEVNULL, check=True)
            check(f"the operator places train {loco} in {block}",
                  wait_for(node_log, rf"EVENT\s+train {loco} placed in block {block}", 5))

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
        time.sleep(25)
        throttle.sendall(message(0xA0, slot, 0))
        time.sleep(2)

        sim, node = read(sim_log), read(node_log)
        start = sim.index("track power ON")
        truth = re.findall(r"block (\S+) occupied \(LocoNet sensor \d+\)", sim[start:])
        followed = re.findall(rf"train {LOCO} entered block (\S+)", node)
        check(f"the simulator moved 4444 through blocks ({', '.join(truth)})", len(truth) >= 2)
        check(f"the node followed 4444 into the same blocks ({', '.join(followed)})", followed == truth)
        check("7701 was not moved", not re.search(r"train 7701 (entered|left)", node))
        check("no block was ambiguous", "AMBIGUOUS" not in node)
        # Before the operator placed them every occupied block is a ghost; afterwards, only
        # those of the loco nobody placed, and no new ones.
        placed = node.index("train 7701 placed")
        before = set(re.findall(r"GHOST block (\S+) occupied", node[:placed]))
        after = set(re.findall(r"GHOST block (\S+) occupied", node[placed:]))
        check(f"before placing, every occupied block was a ghost ({len(before)})", len(before) == 6)
        check(f"after placing, no new ghosts ({', '.join(sorted(after)) or 'none'})", not after)
        check("5555's blocks stay unclaimed",
              not re.search(r"train \d+ (placed in|entered) block timo-adorf/adorf_3_", node))
        last = truth[-1] if truth else "?"
        check(f"Windows' node has MacBook's train picture, with 4444 in {last}",
              wait_for(other_log, rf"<- MacBook's trains, version \d+: .*{LOCO} in {re.escape(last)}", 5))
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
