#!/usr/bin/env python3
"""Step 1 of the district simulators (wiki "District Simulators and Train Tracking"): one
district's world with a real node on it, and a throttle that drives a loco.

The district simulator for MacBook (examples/layout/bench-m5.json) with loco 4444 on Adorf's
platform 1, and a node that joins it as a Hive-Rail node board (DCC-EX on TCP, the board's
LocoNet buffer). A LocoNet throttle on the node switches the district's power on, takes loco
4444 and drives it out of the station, throws turnout 3 (LocoNet switch 21) and stops the
loco. The node is unchanged: it does not know it runs on a simulator.

    cargo build && python3 tools/district_sim_step1.py [target/debug/hive-rail]
"""
import re
from district_sim_harness import read, run, wait_for

LOCO = 4444


def scenario(bench):
    sim = bench.log("sim-MacBook")
    bench.sim("MacBook", bodies=[f"{LOCO}@timo-adorf/1,heading=backward,along=900,length=600,kmh=100"])
    bench.node("MacBook")
    bench.check("the node joins the simulator's DCC-EX and LocoNet buffer", bench.joined("MacBook"))
    throttle = bench.throttle("MacBook")
    throttle.power(True)
    bench.check("power on from the throttle reaches the simulator's track", wait_for(sim, r"track power ON", 5))
    slot = throttle.take(LOCO)
    if not bench.check(f"the node gives loco {LOCO} a slot", slot is not None):
        return
    throttle.drive(slot, 64)
    bench.check("the loco runs in the simulator at the node's speed",
                wait_for(sim, rf"loco {LOCO} at .* decoder [1-9]\d* km/h forward", 15))
    bench.check("its detectors report the blocks it runs into",
                wait_for(sim, r"block timo-\S+ occupied \(LocoNet sensor \d+\)", 20))
    sensors = set(re.findall(r"occupied \(LocoNet sensor (\d+)\)", read(sim)))
    bench.check(f"the node receives those reports on its LocoNet bus ({', '.join(sorted(sensors))})",
                sensors and all(wait_for(bench.log("node-MacBook"), rf"sensor {s} high", 5) for s in sensors))
    throttle.switch(21, closed=False)
    bench.check("turnout 3 (LocoNet switch 21) thrown from the throttle moves in the simulator",
                wait_for(sim, r"turnout timo-adorf/3 thrown \(LocoNet switch 21\)", 5))
    throttle.drive(slot, 0)
    bench.check("the loco stops when the throttle stops it",
                wait_for(sim, rf"loco {LOCO} at \S+ \d+ mm, decoder 0 km/h", 15))


if __name__ == "__main__":
    run("step1", scenario)
