#!/usr/bin/env python3
"""Bench test of the three-district simulation (examples/layout/bench-m5.json).

Injects shorts, district and global stop and go, and fast clock changes into the districts
(MacBook and Windows through their nodes' key pipes, the M5StickC lite node through
`hive-rail --sim-hw`), and checks the web simulator's live state: which trains stand and which
move. Set-up (see the wiki page "Node-Lite on the M5StickC"):

    mkfifo $FIFO_DIR/macbook.fifo $FIFO_DIR/windows.fifo   # each kept open by a writer
    hive-rail --mode node --session bench --id MacBook --rocnet-address 101 --plain \
        --lite-server --layout examples/layout/bench-m5.json < $FIFO_DIR/macbook.fifo
    hive-rail --mode node --session bench --id Windows --rocnet-address 102 --plain \
        --lite-server --listen tcp/0.0.0.0:7448 --layout examples/layout/bench-m5.json < $FIFO_DIR/windows.fifo
    hive-rail --web-sim examples/layout/bench-m5.json --port 8080 --session bench
    # and the M5StickC running firmware/node-lite in session bench

    FIFO_DIR=... python3 tools/bench_m5_test.py
"""
import json, os, subprocess, sys, time, urllib.request

SP = os.environ.get("FIFO_DIR", ".")
HR = os.environ.get("HIVE_RAIL", os.path.join(os.path.dirname(__file__), "..", "target", "debug", "hive-rail"))
results = []


def state():
    with urllib.request.urlopen("http://localhost:8080/api/state", timeout=3) as r:
        return json.load(r)


def trains():
    return {t["name"]: t for t in state()["trains"]}


def key(node, k):
    with open(f"{SP}/{node}.fifo", "w") as f:
        f.write(k + "\n")


def hw(action):
    subprocess.run([HR, "--sim-hw", "M5StickC", action, "--session", "bench"], capture_output=True, timeout=20)


def heads():
    return {n: tuple(t["points"][0]) if t["points"] else None for n, t in trains().items()}


def moving(seconds=2.0):
    """Which trains moved over the next seconds."""
    a = heads()
    time.sleep(seconds)
    b = heads()
    return {n: a[n] != b[n] for n in a}


def wait(condition, what, timeout=90):
    end = time.time() + timeout
    while time.time() < end:
        t = trains()
        if condition(t):
            return t
        time.sleep(0.5)
    raise RuntimeError(f"timeout waiting for {what}: {[(n, x['state'], x['district']) for n, x in trains().items()]}")


def check(name, ok, detail):
    results.append((name, ok, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {name}: {detail}", flush=True)


def all_on():
    key("macbook", "1"); key("windows", "1"); hw("power_on")
    time.sleep(2)


def district_test(name, district, fault, restore):
    """A fault in one district stops only the trains there; restoring it lets them go on."""
    t = wait(lambda t: any(x["district"] == district and x["state"] == "Running" for x in t.values()),
             f"a running train in {district}")
    inside = [n for n, x in t.items() if x["district"] == district]
    fault()
    time.sleep(1.5)
    t = trains()
    mv = moving()
    stopped_inside = all(t[n]["state"] == "NoPower" and not mv[n] for n in inside)
    others = [n for n in t if t[n]["district"] != district and t[n]["state"] == "Running"]
    others_move = all(mv[n] for n in others)
    check(f"{name}: trains in {district} stop", stopped_inside, f"{inside} -> {[t[n]['state'] for n in inside]}")
    check(f"{name}: trains elsewhere run on", others_move, f"{others} moving={[mv[n] for n in others]}")
    restore()
    time.sleep(2)
    mv = moving()
    t = trains()
    check(f"{name}: power back, they go on", all(t[n]["state"] != "NoPower" for n in inside) and any(mv[n] for n in inside),
          f"{[(n, t[n]['state'], mv[n]) for n in inside]}")


def global_test(name, stop, go):
    wait(lambda t: all(x["state"] == "Running" for x in t.values()), "all trains running")
    stop()
    time.sleep(2)
    t = trains()
    mv = moving()
    check(f"{name}: global stop halts every train", all(x["state"] == "NoPower" for x in t.values()) and not any(mv.values()),
          f"{[(n, x['state'], x['district']) for n, x in t.items()]}")
    go()
    time.sleep(2)
    t = trains()
    mv = moving()
    check(f"{name}: global go, every train runs again", all(x["state"] != "NoPower" for x in t.values()) and all(mv.values()),
          f"{[(n, x['state'], mv[n]) for n, x in t.items()]}")


def main():
    all_on()
    district_test("T1 short in the M5StickC (injected)", "M5StickC", lambda: hw("short"), lambda: hw("power_on"))
    district_test("T2 short in Windows (key s)", "Windows", lambda: key("windows", "s"), lambda: key("windows", "1"))
    district_test("T3 district stop in MacBook (key h)", "MacBook", lambda: key("macbook", "h"), lambda: key("macbook", "1"))
    global_test("T4 global stop from Windows, go from MacBook", lambda: key("windows", "E"), lambda: key("macbook", "G"))
    global_test("T5 global stop and go from the M5StickC", lambda: hw("global_stop"), lambda: hw("global_go"))

    # T6: a short stays through a global go; judged by each train's district when checked.
    wait(lambda t: any(x["district"] == "M5StickC" for x in t.values()), "a train in the M5StickC district")
    hw("short"); time.sleep(1.5)
    key("windows", "E"); time.sleep(1.5)
    key("windows", "G"); time.sleep(2.5)
    t = trains()
    inside = [n for n, x in t.items() if x["district"] == "M5StickC"]
    others = [n for n, x in t.items() if x["district"] != "M5StickC"]
    check("T6 a short stays through a global go", all(t[n]["state"] == "NoPower" for n in inside),
          f"{[(n, t[n]['state']) for n in inside]}")
    check("T6 the other districts run again", all(t[n]["state"] != "NoPower" for n in others),
          f"{[(n, t[n]['state'], t[n]['district']) for n in others]} (empty: all trains were in the shorted district)")
    hw("power_on"); time.sleep(2)

    # T7: fast clock run/pause and rate from a node; the simulator follows the mesh's clock.
    # A rate change while the clock is paused is not sent: RocNet's clock frame says "paused" as
    # divider 0, so the rate is changed while it runs.
    c0 = state()["clock"]
    key("macbook", "c"); time.sleep(2)
    c1 = state()["clock"]
    check("T7 clock run/pause from MacBook reaches the simulator", c1["running"] != c0["running"], f"{c0} -> {c1}")
    if not c1["running"]:
        key("macbook", "c"); time.sleep(2); c1 = state()["clock"]
    key("macbook", "+"); time.sleep(2)
    c2 = state()["clock"]
    check("T7 clock rate (running) from MacBook reaches the simulator", c2["rate"] == c1["rate"] + 1, f"{c1} -> {c2}")
    key("macbook", "-"); time.sleep(1)

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)} of {len(results)} checks passed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
