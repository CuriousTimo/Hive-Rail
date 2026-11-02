#!/usr/bin/env python3
"""A lite node's district adopted by a full node (wiki "District Simulators and Train
Tracking"). MacBook is a full node with the layout; the M5StickC is a lite node carried by
MacBook (`--lite-via MacBook`): no train tracking of its own, its detectors reported as
RocNet sensors. MacBook adopts its district: it follows the trains there, takes them in and
hands them back at the border, and holds them before a border that is not clear, which the
lite node applies on its command station. An operator runs train 4444 from Cdorf to the
M5StickC's t1 and back; MacBook is switched off while the train stands in the M5StickC's
district, so it must wait there.

    cargo build && python3 tools/district_sim_step9.py [target/debug/hive-rail]
"""
import re, time
from district_sim_harness import read, run, wait_for
from district_sim_step2 import LOCO, two_districts
from district_sim_step7 import A, B


def scenario(bench):
    throttles = two_districts(bench, layout=True, m5_lite=True,
                              bodies=[f"{LOCO}@timo-cdorf/1,heading=forward,along=1475,length=300,kmh=80"])
    macbook, lite = bench.log("node-MacBook"), bench.log("node-M5StickC")
    bench.check("MacBook adopts the M5StickC's district",
                wait_for(macbook, r"EVENT\s+Adopting district M5StickC: its lite node reaches the mesh through this node", 10))
    bench.check("the lite node follows no trains itself", "Own track" not in read(lite))
    bench.place(LOCO, A)
    bench.operators([f"{LOCO}@{A},{B},speed=64,dwell=6"])
    ops = bench.log("operators")
    bench.check("the train runs to the M5StickC's district",
                wait_for(ops, rf"train {LOCO} leaves for {B}", 15))
    bench.check("MacBook follows it into the adopted district",
                wait_for(macbook, rf"EVENT\s+train {LOCO} came in from MacBook into block m5-2e96-in/t1", 40))
    bench.check("and puts the loco in the M5StickC's district",
                wait_for(macbook, rf"EVENT\s+Loco {LOCO} is in M5StickC now: its lite node's command station drives it", 5))
    bench.check("the lite node's DCC-EX drives it",
                wait_for(lite, rf"\[DCC-EX\] -> <t {LOCO} [1-9]\d* 1>", 10))
    bench.check("it arrives at the M5StickC's t1", wait_for(ops, rf"train {LOCO} arrived at {B}", 40))
    throttles["MacBook"].power(False)
    bench.check("with MacBook off, MacBook holds the train in the adopted district",
                wait_for(macbook, rf"EVENT\s+STOP train {LOCO} held before the border to MacBook \(m5-2e96-in/t1\): MacBook is OFF", 10))
    mark = len(read(lite))
    bench.check("the lite node applies the hold on its command station",
                wait_for(lite, rf"EVENT\s+STOP train {LOCO} held", 5)
                and wait_for(lite, rf"\[DCC-EX\] -> <t {LOCO} 0 ", 10, after=0))
    time.sleep(8)
    bench.check("the train does not cross while MacBook is off",
                not re.search(rf"train {LOCO} came in from M5StickC", read(macbook)))
    throttles["MacBook"].power(True)
    bench.check("with MacBook on again, the hold ends",
                wait_for(lite, rf"EVENT\s+PROCEED train {LOCO}", 10, after=mark))
    bench.check("and MacBook takes the train back",
                wait_for(macbook, rf"EVENT\s+train {LOCO} came in from M5StickC", 40))
    bench.check("it arrives back at Cdorf", wait_for(ops, rf"train {LOCO} arrived at {A}", 40))
    for district in ("MacBook", "M5StickC"):
        bench.check(f"{district}'s node saw no ghost and nothing ambiguous after placing",
                    not re.search(r"GHOST|AMBIGUOUS", read(bench.log(f"node-{district}"))[read(bench.log(f"node-{district}")).find("placed in block"):]))
        bench.check(f"{district}'s simulator saw no fault and no train into a district without power",
                    not re.search(r"FAULT|INVARIANT", read(bench.log(f"sim-{district}"))))


if __name__ == "__main__":
    run("step9", scenario)
