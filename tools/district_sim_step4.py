#!/usr/bin/env python3
"""Step 4 of the district simulators (wiki "District Simulators and Train Tracking"):
following trains on the node. MacBook's simulator has three locos in Adorf: 4444 on
platform 1, 7701 on platform 2 and 5555 on platform 3. An operator places 4444 and 7701
with `--place-train`; 5555 is never placed. A throttle drives 4444 out. Against the
simulator's own log (the truth): the node follows 4444 into exactly the blocks it runs
into, 7701 stays, 5555's blocks stay ghosts, nothing is ambiguous, and a second node
(Windows) gets MacBook's train picture over the mesh.

    cargo build && python3 tools/district_sim_step4.py [target/debug/hive-rail]
"""
import re, time
from district_sim_harness import read, run, wait_for

LOCO = 4444


def scenario(bench):
    node = bench.log("node-MacBook")
    bodies = [f"{loco}@timo-adorf/{platform},heading=backward,along=900,length=500,kmh=100"
              for loco, platform in ((LOCO, 1), (7701, 2), (5555, 3))]
    bench.sim("MacBook", bodies=bodies)
    bench.node("Windows", layout=True, board=False)
    bench.node("MacBook", layout=True)
    bench.check("the node learns the occupied blocks (interrogate)",
                wait_for(node, r"EVENT\s+block timo-adorf/adorf_3_\w+ occupied", 15))
    bench.check("the blocks of a loco nobody placed are ghosts",
                wait_for(node, r"GHOST block timo-adorf/adorf_3_\w+ occupied", 5))
    for loco, block in ((LOCO, "timo-adorf/adorf_1_b"), (7701, "timo-adorf/adorf_2_b")):
        bench.place(loco, block)
        bench.check(f"the operator places train {loco} in {block}",
                    wait_for(node, rf"EVENT\s+train {loco} placed in block {block}", 5))
    throttle = bench.throttle("MacBook")
    throttle.power(True)
    slot = throttle.take(LOCO)
    if not bench.check(f"the node gives loco {LOCO} a slot", slot is not None):
        return
    throttle.drive(slot, 64)
    time.sleep(25)
    throttle.drive(slot, 0)
    time.sleep(2)

    sim, text = read(bench.log("sim-MacBook")), read(node)
    truth = re.findall(r"block (\S+) occupied \(LocoNet sensor \d+\)", sim[sim.index("track power ON"):])
    followed = re.findall(rf"train {LOCO} entered block (\S+)", text)
    bench.check(f"the simulator moved 4444 through blocks ({', '.join(truth)})", len(truth) >= 2)
    bench.check(f"the node followed 4444 into the same blocks ({', '.join(followed)})", followed == truth)
    bench.check("7701 was not moved", not re.search(r"train 7701 (entered|left)", text))
    bench.check("no block was ambiguous", "AMBIGUOUS" not in text)
    # Before the operator placed them every occupied block is a ghost; afterwards, only
    # those of the loco nobody placed, and no new ones.
    placed = text.index("train 7701 placed")
    before = set(re.findall(r"GHOST block (\S+) occupied", text[:placed]))
    after = set(re.findall(r"GHOST block (\S+) occupied", text[placed:]))
    bench.check(f"before placing, every occupied block was a ghost ({len(before)})", len(before) == 6)
    bench.check(f"after placing, no new ghosts ({', '.join(sorted(after)) or 'none'})", not after)
    bench.check("5555's blocks stay unclaimed",
                not re.search(r"train \d+ (placed in|entered) block timo-adorf/adorf_3_", text))
    last = truth[-1] if truth else "?"
    bench.check(f"Windows' node has MacBook's train picture, with 4444 in {last}",
                wait_for(bench.log("node-Windows"), rf"<- MacBook's trains, version \d+: .*{LOCO} in {re.escape(last)}", 5))


if __name__ == "__main__":
    run("step4", scenario)
