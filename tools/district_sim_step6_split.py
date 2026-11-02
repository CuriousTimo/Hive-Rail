#!/usr/bin/env python3
"""Step 6, a fault: a network split cuts the M5StickC's node off (frozen: it sends and
answers nothing) before the train reaches the border, and the network joins again (see
district_sim_step6_faults.py).

    cargo build && python3 tools/district_sim_step6_split.py [target/debug/hive-rail]
"""
from district_sim_harness import run
from district_sim_step6_faults import neighbour_gone

if __name__ == "__main__":
    run("step6-split", neighbour_gone(lambda bench: bench.freeze("node-M5StickC"),
                                      lambda bench: bench.thaw("node-M5StickC"), "M5StickC is not heard from"))
