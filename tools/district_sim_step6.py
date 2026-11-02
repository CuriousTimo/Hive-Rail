#!/usr/bin/env python3
"""Step 6 of the district simulators (wiki "District Simulators and Train Tracking"):
default Stop at the border. As step 5, but the M5StickC's district is switched off first:
MacBook's node holds the train before the border at speed 0 although the throttle drives
it, and lets it go once the M5StickC has power again; neither simulator sees a train run
into a district without power.

    cargo build && python3 tools/district_sim_step6.py [target/debug/hive-rail]
"""
import re, time
from district_sim_harness import read, run, wait_for
from district_sim_step2 import LOCO, two_districts
from district_sim_step5 import placed


def scenario(bench):
    throttles = two_districts(bench, layout=True)
    node = bench.log("node-MacBook")
    throttles["M5StickC"].power(False)
    bench.check("the M5StickC's district is off", wait_for(bench.log("sim-M5StickC"), r"track power OFF", 5))
    time.sleep(1)  # MacBook hears it in the M5StickC's heartbeat
    slot = placed(bench, throttles)
    if slot is None:
        return
    throttles["MacBook"].drive(slot, 64)
    bench.check("MacBook's node holds the train before the border: the M5StickC has no power",
                wait_for(node, rf"EVENT\s+STOP train {LOCO} held before the border to M5StickC \(timo-2e96-border/edge_1\): M5StickC is OFF", 5))
    time.sleep(3)
    text = read(node)
    bench.check("its DCC-EX keeps the loco at speed 0 although the throttle drives it",
                re.search(rf"\[DCC-EX\] -> <t {LOCO} 0 1>", text) and not re.search(rf"\[DCC-EX\] -> <t {LOCO} [1-9]\d* 1>", text))
    time.sleep(8)
    bench.check("the train does not cross", f"loco {LOCO} runs on into" not in read(bench.log("sim-MacBook")))
    bench.check("and stands in MacBook's border block", bench.status("MacBook", LOCO)[::2] == ("timo-2e96-border/1", "0"))
    throttles["M5StickC"].power(True)
    bench.check("once the M5StickC has power again the border is clear",
                wait_for(node, rf"EVENT\s+PROCEED train {LOCO}: the border is clear", 5))
    bench.check("and the loco gets its speed back", wait_for(node, rf"\[DCC-EX\] -> <t {LOCO} [1-9]\d* 1>", 5))
    bench.check("it crosses, and the M5StickC's node takes it",
                wait_for(bench.log("node-M5StickC"), rf"EVENT\s+train {LOCO} came in from MacBook", 30))
    throttles["MacBook"].drive(slot, 0)
    for district in ("MacBook", "M5StickC"):
        bench.check(f"{district}'s simulator saw no train run into a district without power",
                    "INVARIANT VIOLATED" not in read(bench.log(f"sim-{district}")))


if __name__ == "__main__":
    run("step6", scenario)
