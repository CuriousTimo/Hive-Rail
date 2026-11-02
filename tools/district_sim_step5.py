#!/usr/bin/env python3
"""Step 5 of the district simulators (wiki "District Simulators and Train Tracking"): the
handoff. As step 2, but the nodes know their track (`--layout`) and an operator placed the
train. At the border the M5StickC's node takes the train from MacBook's picture and the loco
into its district (its DCC-EX drives it), MacBook's node lets go once its border block is
free, and MacBook's throttle still drives the loco there.

    cargo build && python3 tools/district_sim_step5.py [target/debug/hive-rail]
"""
import re
from district_sim_harness import read, run, wait_for
from district_sim_step2 import LOCO, two_districts


def placed(bench, throttles):
    """Train 4444 placed in MacBook's border block and taken on MacBook's throttle."""
    bench.place(LOCO, "timo-2e96-border/edge_1")
    bench.check("MacBook's node follows the train it was given",
                wait_for(bench.log("node-MacBook"), rf"EVENT\s+train {LOCO} placed in block timo-2e96-border/edge_1", 5))
    slot = throttles["MacBook"].take(LOCO)
    bench.check(f"MacBook's node gives loco {LOCO} a slot", slot is not None)
    return slot


def no_ghosts(bench):
    for district in ("MacBook", "M5StickC"):
        text = read(bench.log(f"node-{district}"))
        bench.check(f"{district}'s node saw no ghost and nothing ambiguous after placing",
                    not re.search(r"GHOST|AMBIGUOUS", text[text.find("placed in block"):]))


def scenario(bench):
    throttles = two_districts(bench, layout=True)
    m5_node = bench.log("node-M5StickC")
    slot = placed(bench, throttles)
    if slot is None:
        return
    throttles["MacBook"].drive(slot, 64)
    bench.check("MacBook's simulator hands the loco over at the border",
                wait_for(bench.log("sim-MacBook"), rf"loco {LOCO} runs on into M5StickC", 30))
    bench.check("the M5StickC's node takes the train from MacBook's picture",
                wait_for(m5_node, rf"EVENT\s+train {LOCO} came in from MacBook into block m5-\S+", 5))
    bench.check("and takes the loco into its district",
                wait_for(m5_node, rf"EVENT\s+Loco {LOCO} is in this district now", 5))
    bench.check("the M5StickC's DCC-EX drives it now (the gap of step 2 is closed)",
                wait_for(m5_node, rf"\[DCC-EX\] -> <t {LOCO} [1-9]\d* 1>", 5))
    bench.check("MacBook's node hands the train over once its border block is free",
                wait_for(bench.log("node-MacBook"), rf"EVENT\s+train {LOCO} handed over to M5StickC", 15))
    throttles["MacBook"].drive(slot, 0)
    bench.check("stopping it on MacBook's throttle reaches the M5StickC's DCC-EX",
                wait_for(m5_node, rf"\[DCC-EX\] -> <t {LOCO} 0 1>", 5))
    bench.check("and the loco stops in the M5StickC's district",
                wait_for(bench.log("sim-M5StickC"), rf"loco {LOCO} at m5-\S+ \d+ mm, decoder 0 km/h", 15))
    no_ghosts(bench)


if __name__ == "__main__":
    run("step5", scenario)
