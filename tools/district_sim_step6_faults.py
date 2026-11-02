"""The faults of step 6 (wiki "District Simulators and Train Tracking"): the neighbour gone.
Two districts as in step 5, train 4444 placed at MacBook's border, and the M5StickC's node
taken away before the train sets off, in one of two ways (`lost`: it dies and is started
again; `split`: it is frozen, as on the other side of a network split, and runs on). While
it is away, MacBook holds the train and shows Hp 0; once it is back, the border clears and
the train crosses.
"""
import time
from district_sim_harness import read, wait_for
from district_sim_step2 import LOCO, two_districts
from district_sim_step5 import placed


def neighbour_gone(away, back, reason):
    def scenario(bench):
        throttles = two_districts(bench, layout=True)
        node = bench.log("node-MacBook")
        slot = placed(bench, throttles)
        if slot is None:
            return
        away(bench)
        time.sleep(1.5)  # MacBook calls a silent peer stale after 750 ms
        throttles["MacBook"].drive(slot, 64)
        bench.check(f"MacBook's node holds the train: {reason}",
                    wait_for(node, rf"EVENT\s+STOP train {LOCO} held before the border to M5StickC \(timo-2e96-border/edge_1\): {reason}", 5))
        bench.check("and shows Hp 0 at the border",
                    wait_for(bench.log("sim-MacBook"), r"signal timo-2e96-border/signal_13 shows Hp 0", 5))
        time.sleep(6)
        bench.check("the train does not cross while the M5StickC is away",
                    f"loco {LOCO} runs on into" not in read(bench.log("sim-MacBook")))
        back(bench)
        bench.check("once it is back the border is clear",
                    wait_for(node, rf"EVENT\s+PROCEED train {LOCO}: the border is clear", 20))
        bench.check("and the M5StickC's node takes the train",
                    wait_for(bench.log("node-M5StickC"), rf"EVENT\s+train {LOCO} came in from MacBook", 30))
        throttles["MacBook"].drive(slot, 0)
        for district in ("MacBook", "M5StickC"):
            bench.check(f"{district}'s simulator saw no train run into a district without power",
                        "INVARIANT VIOLATED" not in read(bench.log(f"sim-{district}")))
    return scenario
