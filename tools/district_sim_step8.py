#!/usr/bin/env python3
"""Step 8 of the district simulators (wiki "District Simulators and Train Tracking"): the
web view from the mesh (`--web-mesh`). The bench of step 7 (two districts, an operator
running train 4444 back and forth over the border) with the web view drawn from what the
nodes publish, and the simulators' truth over it (`--truth`). Reading the page's live state
for a while, checks that it shows the nodes' blocks, the boundary signal and their events,
and that the train the nodes follow is in the district where the simulators have it.

    cargo build && python3 tools/district_sim_step8.py [target/debug/hive-rail]
"""
import urllib.request
from district_sim_harness import run, snapshots, wait_for
from district_sim_step2 import LOCO, two_districts
from district_sim_step7 import A, B

PORT = 8090


def scenario(bench):
    two_districts(bench, layout=True, bodies=[f"{LOCO}@timo-cdorf/1,heading=forward,along=1475,length=300,kmh=80"])
    bench.web_mesh(PORT, truth=True)
    bench.check("the web view starts", wait_for(bench.log("web-mesh"), r"drawn from session", 10))
    bench.place(LOCO, A)
    bench.operators([f"{LOCO}@{A},{B},speed=64,dwell=3"])
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/api/layout", timeout=5) as page:
        bench.check("it serves the layout to draw", page.status == 200 and b'"modules"' in page.read())
    states = snapshots(PORT, 45)
    bench.check(f"it sends the live state ({len(states)} states in 45 s)", len(states) > 100)

    def trains(state, prefix):
        return [t for t in state["trains"] if t["name"].startswith(prefix) and t["address"] == LOCO]

    followed = [s for s in states if trains(s, "train ")]
    bench.check("it shows the train the nodes follow", followed)
    districts = {trains(s, "train ")[0]["district"] for s in followed}
    bench.check(f"in both districts as it runs ({', '.join(sorted(districts))})", districts == {"MacBook", "M5StickC"})
    bench.check("it shows the simulators' train over it (--truth)",
                any(trains(s, "simulator:") and trains(s, "simulator:")[0]["points"] for s in states))
    # Where both show it, the nodes' district is the simulators' most of the time (they
    # differ only for the moment of a handover).
    both = [s for s in states if trains(s, "train ") and trains(s, "simulator:")]
    agree = sum(trains(s, "train ")[0]["district"] in {t["district"] for t in trains(s, "simulator:")} for s in both)
    bench.check(f"the nodes have it where the simulators have it ({agree} of {len(both)} states)",
                both and agree >= 0.9 * len(both))
    bench.check("it shows the blocks the nodes report occupied", any(s["occupied"] for s in states))
    bench.check("it shows the boundary signal as MacBook set it",
                any(s["signals"].get("timo-2e96-border/signal_13") == "green" for s in states))
    bench.check("its log has the nodes' events",
                any(any(f"train {LOCO} came in from" in line for line in s["log"]) for s in states))
    bench.check("and the fast clock", all("clock" in s for s in states))


if __name__ == "__main__":
    run("step8", scenario)
