#!/usr/bin/env python3
"""Step 2 of the district simulators (wiki "District Simulators and Train Tracking"): the
border. Two district simulators, MacBook and M5StickC, on one world channel, each with a
real node on it as on a node board. Loco 4444 stands on MacBook's border module; a throttle
on MacBook's node drives it towards the M5StickC. It crosses at its last speed, though the
M5StickC's node never drove it: the gap that steps 4 and 5 close.

    cargo build && python3 tools/district_sim_step2.py [target/debug/hive-rail]
"""
import re
from district_sim_harness import read, run, wait_for

LOCO = 4444
BODY = f"{LOCO}@timo-2e96-border/1,heading=forward,along=300,length=300,kmh=80"


def two_districts(bench, layout=False, bodies=(BODY,), m5_lite=False):
    """MacBook (with loco 4444 at its border, or the given bodies) and the M5StickC, both
    powered; MacBook's throttle, and the M5StickC's. With `m5_lite`, the M5StickC is a
    lite node carried by MacBook."""
    bench.sim("MacBook", bodies=list(bodies), world=True)
    bench.sim("M5StickC", world=True)
    bench.node("MacBook", layout=layout)
    # The M5StickC as a lite node carried by MacBook, or as a full node.
    bench.node("M5StickC", layout=layout and not m5_lite, lite_via="MacBook" if m5_lite else None)
    bench.check("the simulators find each other on the world channel",
                wait_for(bench.log("sim-MacBook"), r"M5StickC's simulator is on the world channel", 15)
                and wait_for(bench.log("sim-M5StickC"), r"MacBook's simulator is on the world channel", 15))
    throttles = {}
    for district in ("MacBook", "M5StickC"):
        bench.check(f"{district}'s node joins its simulator", bench.joined(district))
        throttles[district] = bench.throttle(district)
        throttles[district].power(True)
        bench.check(f"{district}'s track has power", wait_for(bench.log(f"sim-{district}"), r"track power ON", 5))
    return throttles


def scenario(bench):
    throttles = two_districts(bench)
    macbook, m5_sim, m5_node = throttles["MacBook"], bench.log("sim-M5StickC"), bench.log("node-M5StickC")
    slot = macbook.take(LOCO)
    if not bench.check(f"MacBook's node gives loco {LOCO} a slot", slot is not None):
        return
    macbook.drive(slot, 64)
    bench.check("MacBook's simulator hands the loco over at the border",
                wait_for(bench.log("sim-MacBook"), rf"loco {LOCO} runs on into M5StickC", 30))
    bench.check("the M5StickC's simulator takes it", wait_for(m5_sim, rf"loco {LOCO} came in from MacBook", 5))
    bench.check("it runs on in the M5StickC's district at its last speed",
                wait_for(m5_sim, rf"loco {LOCO} at m5-\S+ \d+ mm, decoder [1-9]\d* km/h forward", 15))
    bench.check("the M5StickC's node never drove it (the gap steps 4 and 5 close)",
                not re.search(rf"<t {LOCO} ", read(m5_node)))
    bench.check("the M5StickC's detectors report it",
                wait_for(m5_sim, r"block m5-\S+ occupied \(LocoNet sensor \d+\)", 15))
    sensors = set(re.findall(r"occupied \(LocoNet sensor (\d+)\)", read(m5_sim)))
    bench.check(f"the M5StickC's node receives the reports ({', '.join(sorted(sensors))})",
                sensors and all(wait_for(m5_node, rf"sensor {s} high", 5) for s in sensors))
    bench.check("MacBook's border blocks go free once the train has left them",
                wait_for(bench.log("sim-MacBook"), r"block timo-2e96-border/\S+ free", 15))
    throttles["M5StickC"].power(False)
    bench.check("switching the M5StickC's district off reaches its track", wait_for(m5_sim, r"track power OFF", 5))
    bench.check("the loco stands without power",
                wait_for(m5_sim, rf"loco {LOCO} at .*, no track power", 15))


if __name__ == "__main__":
    run("step2", scenario)
