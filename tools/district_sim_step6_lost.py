#!/usr/bin/env python3
"""Step 6, a fault: the M5StickC's node dies before the train reaches the border, and is
started again (see district_sim_step6_faults.py).

    cargo build && python3 tools/district_sim_step6_lost.py [target/debug/hive-rail]
"""
from district_sim_harness import run
from district_sim_step6_faults import neighbour_gone


def restart(bench):
    bench.node("M5StickC", layout=True)


if __name__ == "__main__":
    run("step6-lost", neighbour_gone(lambda bench: bench.stop("node-M5StickC"), restart, "M5StickC is not heard from"))
