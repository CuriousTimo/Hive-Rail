#!/usr/bin/env python3
"""Step 1 of the district simulators (wiki "District Simulators and Train Tracking"): one
district's world with a real node on it, and a throttle that drives a loco.

Starts the district simulator for MacBook (examples/layout/bench-m5.json) with loco 4444 on
Adorf's platform 1, and a node that joins it as a Hive-Rail node board (DCC-EX on TCP, the
board's LocoNet buffer). A LocoNet throttle on the node's LocoNet over TCP port then:

  1. switches the district's power on,
  2. takes loco 4444 and drives it out of the station,
  3. throws turnout 3 (LocoNet switch 21),
  4. stops the loco,

and checks both logs: the simulator moved the loco at the node's speed and its detectors
reported the blocks, the node received those reports on its LocoNet bus, and the turnout
moved. The node is unchanged: it does not know it runs on a simulator.

    cargo build && python3 tools/district_sim_step1.py [target/debug/hive-rail]

Exits non-zero if a check fails; the logs are kept in a temporary folder.
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
DCCEX_PORT, BUS_PORT, THROTTLE_PORT = 25600, 12350, 12351
LOCO = 4444


def message(*body):
    checksum = 0xFF
    for byte in body:
        checksum ^= byte
    return bytes(body) + bytes([checksum])


def wait_for(path, pattern, seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        with open(path, errors="replace") as log:
            if re.search(pattern, log.read()):
                return True
        time.sleep(0.2)
    return False


def read_slot(throttle, address, seconds=5):
    """The slot the node gives `address`, from its OPC_SL_RD_DATA answer."""
    deadline, data = time.time() + seconds, b""
    throttle.settimeout(0.2)
    while time.time() < deadline:
        try:
            data += throttle.recv(1024)
        except socket.timeout:
            continue
        for start in range(len(data) - 13):
            if data[start] == 0xE7 and data[start + 1] == 0x0E:
                frame = data[start:start + 14]
                if len(frame) == 14 and (frame[4] | frame[9] << 7) == address:
                    return frame[2]
    return None


def main():
    logs = tempfile.mkdtemp(prefix="district-sim-")
    sim_log, node_log = os.path.join(logs, "sim.log"), os.path.join(logs, "node.log")
    processes = []
    checks = []

    def check(what, ok):
        checks.append((what, ok))
        print(("PASS  " if ok else "FAIL  ") + what)

    try:
        processes.append(subprocess.Popen(
            [BINARY, "--sim-district", LAYOUT, "--district", "MacBook",
             "--body", f"{LOCO}@timo-adorf/1,heading=backward,along=900,length=600,kmh=100",
             "--dccex-port", str(DCCEX_PORT), "--loconet-port", str(BUS_PORT)],
            stdout=open(sim_log, "w"), stderr=subprocess.STDOUT))
        if not wait_for(sim_log, r"join it with", 10):
            check("the district simulator starts", False)
            return
        processes.append(subprocess.Popen(
            [BINARY, "--mode", "node", "--session", "district-sim-step1", "--id", "MacBook",
             "--rocnet-address", "101", "--plain",
             "--dccex", f"127.0.0.1:{DCCEX_PORT}",
             "--loconet-connect", f"127.0.0.1:{BUS_PORT}", "--loconet-connect-slots", "node",
             "--loconet-tcp", str(THROTTLE_PORT), "--loconet-tcp-slots", "node"],
            stdin=subprocess.PIPE, stdout=open(node_log, "w"), stderr=subprocess.STDOUT))
        check("the node joins the simulator's DCC-EX",
              wait_for(sim_log, r"DCC-EX client .* connected", 15))
        check("the node joins the simulator's LocoNet buffer",
              wait_for(sim_log, r"LocoNet client .* connected", 15))
        time.sleep(1)

        throttle = socket.create_connection(("127.0.0.1", THROTTLE_PORT), timeout=5)
        throttle.sendall(message(0x83))  # OPC_GPON
        check("power on from the throttle reaches the simulator's track",
              wait_for(sim_log, r"track power ON", 5))

        throttle.sendall(message(0xBF, LOCO >> 7, LOCO & 0x7F))  # OPC_LOCO_ADR
        slot = read_slot(throttle, LOCO)
        check(f"the node gives loco {LOCO} a slot", slot is not None)
        if slot is None:
            return
        throttle.sendall(message(0xBA, slot, slot))  # null move: in use
        throttle.sendall(message(0xA1, slot, 0x10))  # forward, lights
        throttle.sendall(message(0xA0, slot, 64))  # half speed
        check(f"the loco runs in the simulator at the node's speed",
              wait_for(sim_log, rf"loco {LOCO} at .* decoder ([1-9][0-9]*) km/h forward", 15))
        check("its detectors report the blocks it runs into",
              wait_for(sim_log, r"block timo-.* occupied \(LocoNet sensor \d+\)", 20))
        with open(sim_log) as log:
            sensors = re.findall(r"occupied \(LocoNet sensor (\d+)\)", log.read())
        check(f"the node receives those reports on its LocoNet bus ({', '.join(sorted(set(sensors)))})",
              bool(sensors) and all(wait_for(node_log, rf"sensor {s} high", 5) for s in set(sensors)))

        throttle.sendall(message(0xB0, (21 - 1) & 0x7F, 0x10 | ((21 - 1) >> 7)))  # SW_REQ 21 thrown, on
        throttle.sendall(message(0xB0, (21 - 1) & 0x7F, 0x00 | ((21 - 1) >> 7)))  # and off
        check("turnout 3 (LocoNet switch 21) thrown from the throttle moves in the simulator",
              wait_for(sim_log, r"turnout timo-adorf/3 thrown \(LocoNet switch 21\)", 5))

        throttle.sendall(message(0xA0, slot, 0))
        time.sleep(11)  # the next status line
        with open(sim_log) as log:
            last = re.findall(rf"loco {LOCO} at (\S+) (\d+) mm, decoder (\d+) km/h", log.read())[-1]
        check(f"the loco stops when the throttle stops it (at {last[0]} {last[1]} mm)", last[2] == "0")
    except Exception:
        traceback.print_exc()
        check("the test itself runs without an error", False)
    finally:
        for process in reversed(processes):
            process.terminate()
        for process in processes:
            process.wait(5)
        failed = [what for what, ok in checks if not ok]
        print(f"logs in {logs}")
        sys.exit(1 if failed or not checks else 0)


if __name__ == "__main__":
    main()
