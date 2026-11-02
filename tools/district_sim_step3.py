#!/usr/bin/env python3
"""Step 3 of the district simulators (wiki "District Simulators and Train Tracking"): the
node knows its own track. As step 1, but the node runs with `--layout` and a dispatcher
watches: the node asks the detectors for their state (the interrogate), names what its
buses report, and the dispatcher shows it.

    cargo build && python3 tools/district_sim_step3.py [target/debug/hive-rail]
"""
from district_sim_harness import run, wait_for

LOCO = 4444


def scenario(bench):
    sim, node = bench.log("sim-MacBook"), bench.log("node-MacBook")
    bench.sim("MacBook", bodies=[f"{LOCO}@timo-adorf/1,heading=backward,along=900,length=600,kmh=100"])
    bench.dispatcher()
    bench.node("MacBook", layout=True)
    bench.check("the node loads its own district from the layout",
                wait_for(node, r"Own track from .*: district MacBook, \d+ blocks, \d+ turnouts", 10))
    bench.check("the node joins the simulator", bench.joined("MacBook"))
    bench.check("the node asks the detectors (interrogate) and learns where the loco stands",
                wait_for(sim, r"interrogate: every detector reports", 10)
                and wait_for(node, r"EVENT\s+block timo-adorf/adorf_1_b occupied \(LocoNet sensor 203\)", 5))
    throttle = bench.throttle("MacBook")
    throttle.power(True)
    slot = throttle.take(LOCO)
    if not bench.check(f"the node gives loco {LOCO} a slot", slot is not None):
        return
    throttle.drive(slot, 64)
    bench.check("the simulator reports a block",
                wait_for(sim, r"block timo-2e96-west/line_2 occupied \(LocoNet sensor 202\)", 20))
    bench.check("the node names it: block timo-2e96-west/line_2 occupied (LocoNet sensor 202)",
                wait_for(node, r"EVENT\s+block timo-2e96-west/line_2 occupied \(LocoNet sensor 202\)", 5))
    bench.check("and the block it left: block timo-adorf/adorf_1_b free (LocoNet sensor 203)",
                wait_for(node, r"EVENT\s+block timo-adorf/adorf_1_b free \(LocoNet sensor 203\)", 5))
    throttle.switch(21, closed=False)
    bench.check("the node names the switch command: turnout timo-adorf/3 thrown (LocoNet switch 21)",
                wait_for(node, r"EVENT\s+turnout timo-adorf/3 thrown \(LocoNet switch 21\)", 5))
    bench.check("the dispatcher shows the named block in its events",
                wait_for(bench.log("dispatcher"), r"MacBook.*block timo-2e96-west/line_2 occupied", 5))
    throttle.drive(slot, 0)


if __name__ == "__main__":
    run("step3", scenario)
