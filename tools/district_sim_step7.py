#!/usr/bin/env python3
"""Step 7 of the district simulators (wiki "District Simulators and Train Tracking"):
simulated operators. Two districts as in step 5, train 4444 placed in Cdorf (MacBook),
and an operator that runs it back and forth to the M5StickC's t1 through the nodes, as a
person does: it sets the way, leaves, stops, waits and turns back. Checks two round trips:
the train crosses the border and is handed over both ways, it arrives at both stops, and
nothing goes wrong on the way (no ghost, nothing ambiguous, no fault, no train into a
district without power).

    cargo build && python3 tools/district_sim_step7.py [target/debug/hive-rail]
"""
import re
from district_sim_harness import read, run, wait_for
from district_sim_step2 import LOCO, two_districts

A, B = "timo-cdorf/ce_1", "m5-2e96-in/t1"


def scenario(bench):
    two_districts(bench, layout=True, bodies=[f"{LOCO}@timo-cdorf/1,heading=forward,along=1475,length=300,kmh=80"])
    bench.place(LOCO, A)
    bench.check(f"MacBook's node follows train {LOCO} in {A}",
                wait_for(bench.log("node-MacBook"), rf"EVENT\s+train {LOCO} placed in block {A}", 5))
    bench.operators([f"{LOCO}@{A},{B},speed=64,dwell=3"])
    ops = bench.log("operators")
    bench.check("the operator takes the train on MacBook's node",
                wait_for(ops, rf"operator of train {LOCO} takes it on MacBook's node", 10))
    # Each lap looks only at what the logs say after the lap before.
    names = ("operators", "node-MacBook", "node-M5StickC")
    mark = {name: 0 for name in names}
    for lap in (1, 2):
        if lap > 1:
            mark = {name: len(read(bench.log(name))) for name in names}
        seen = lambda name, pattern, seconds: wait_for(bench.log(name), pattern, seconds, after=mark[name])
        bench.check(f"lap {lap}: it leaves for {B}", seen("operators", rf"train {LOCO} leaves for {B}", 15))
        bench.check(f"lap {lap}: the M5StickC's node takes it over",
                    seen("node-M5StickC", rf"EVENT\s+train {LOCO} came in from MacBook", 40))
        bench.check(f"lap {lap}: it arrives at {B}", seen("operators", rf"train {LOCO} arrived at {B}", 40))
        bench.check(f"lap {lap}: it turns back and leaves for {A}", seen("operators", rf"train {LOCO} leaves for {A}", 15))
        bench.check(f"lap {lap}: MacBook's node takes it back",
                    seen("node-MacBook", rf"EVENT\s+train {LOCO} came in from M5StickC", 40))
        bench.check(f"lap {lap}: it arrives at {A}", seen("operators", rf"train {LOCO} arrived at {A}", 40))
    for district in ("MacBook", "M5StickC"):
        node, sim = read(bench.log(f"node-{district}")), read(bench.log(f"sim-{district}"))
        bench.check(f"{district}'s node saw no ghost and nothing ambiguous after placing",
                    not re.search(r"GHOST|AMBIGUOUS", node[node.find("placed in block"):] if district == "MacBook" else node[node.find("Own track"):]))
        bench.check(f"{district}'s simulator saw no fault and no train into a district without power",
                    not re.search(r"FAULT|INVARIANT", sim))


if __name__ == "__main__":
    run("step7", scenario)
