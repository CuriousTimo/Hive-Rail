"""The shared bench of the district simulator tests (tools/district_sim_step1.py to step6.py,
wiki "District Simulators and Train Tracking"): starting district simulators, nodes and a
dispatcher, a LocoNet throttle on a node, reading their logs, and the PASS/FAIL checks.

Each test is a function that gets a `Bench`:

    def scenario(bench):
        bench.sim("MacBook", bodies=["4444@timo-adorf/1,heading=backward,along=900"])
        bench.node("MacBook", layout=True)
        throttle = bench.throttle("MacBook")
        ...
        bench.check("what is expected", condition)

    if __name__ == "__main__":
        run("step1", scenario)

The run exits non-zero when a check fails; the logs stay in a temporary folder.
"""
import os, re, socket, subprocess, sys, tempfile, time, traceback

BINARY = sys.argv[1] if len(sys.argv) > 1 else "target/debug/hive-rail"
LAYOUT = "examples/layout/bench-m5.json"
# Each district's ports: DCC-EX of its simulator, its LocoNet buffer, the node's LocoNet
# over TCP for the throttle; and the node's RocNet address.
DISTRICTS = {
    "MacBook": (25600, 12350, 12351, 101),
    "M5StickC": (25601, 12352, 12353, 199),
    "Windows": (25602, 12354, 12355, 102),
}


def message(*body):
    """A LocoNet message with its checksum."""
    checksum = 0xFF
    for byte in body:
        checksum ^= byte
    return bytes(body) + bytes([checksum])


def read(path):
    with open(path, errors="replace") as log:
        return log.read()


def wait_for(path, pattern, seconds):
    """Whether `pattern` turns up in the log within `seconds`."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        if re.search(pattern, read(path)):
            return True
        time.sleep(0.2)
    return False


class Throttle:
    """A LocoNet throttle on a node's LocoNet over TCP port (binary)."""

    def __init__(self, port):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=5)

    def send(self, *body):
        self.socket.sendall(message(*body))

    def power(self, on):
        self.send(0x83 if on else 0x82)  # OPC_GPON, OPC_GPOFF

    def take(self, address, seconds=5):
        """Asks for a loco and puts it in use: its slot, or None without an answer."""
        self.send(0xBF, address >> 7, address & 0x7F)  # OPC_LOCO_ADR
        deadline, data = time.time() + seconds, b""
        self.socket.settimeout(0.2)
        while time.time() < deadline:
            try:
                data += self.socket.recv(1024)
            except socket.timeout:
                continue
            for start in range(len(data) - 13):
                frame = data[start:start + 14]
                if frame[0] == 0xE7 and frame[1] == 0x0E and (frame[4] | frame[9] << 7) == address:
                    self.send(0xBA, frame[2], frame[2])  # null move: in use
                    return frame[2]
        return None

    def drive(self, slot, speed, forward=True):
        self.send(0xA1, slot, (0 if forward else 0x20) | 0x10)  # direction, lights
        self.send(0xA0, slot, speed)

    def switch(self, turnout, closed):
        wire = turnout - 1
        for on in (0x10, 0x00):
            self.send(0xB0, wire & 0x7F, (0x20 if closed else 0) | on | (wire >> 7))


class Bench:
    def __init__(self, name):
        self.logs = tempfile.mkdtemp(prefix=f"district-sim-{name}-")
        self.session = f"district-sim-{name}-{os.getpid()}"
        self.processes, self.checks = [], []

    def log(self, name):
        return os.path.join(self.logs, f"{name}.log")

    def check(self, what, ok):
        self.checks.append((what, bool(ok)))
        print(("PASS  " if ok else "FAIL  ") + what)
        return ok

    def start(self, name, args, stdin=False):
        self.processes.append(subprocess.Popen(
            [BINARY, *args], stdin=subprocess.PIPE if stdin else None,
            stdout=open(self.log(name), "w"), stderr=subprocess.STDOUT))

    def sim(self, district, bodies=(), world=False):
        """A district simulator; with `world`, on this bench's world channel."""
        dccex, bus, _, _ = DISTRICTS[district]
        args = ["--sim-district", LAYOUT, "--district", district,
                "--dccex-port", str(dccex), "--loconet-port", str(bus)]
        args += [arg for body in bodies for arg in ("--body", body)]
        if world:
            args += ["--session", self.session]
        self.start(f"sim-{district}", args)
        if not wait_for(self.log(f"sim-{district}"), r"join it with", 10):
            raise RuntimeError(f"{district}'s simulator did not start")

    def node(self, district, layout=False, board=True):
        """A node for the district, on its simulator as on a node board, with LocoNet
        over TCP for a throttle; with `layout`, knowing its track."""
        dccex, bus, throttle, rocnet = DISTRICTS[district]
        args = ["--mode", "node", "--session", self.session, "--id", district,
                "--rocnet-address", str(rocnet), "--plain"]
        if layout:
            args += ["--layout", LAYOUT]
        if board:
            args += ["--dccex", f"127.0.0.1:{dccex}",
                     "--loconet-connect", f"127.0.0.1:{bus}", "--loconet-connect-slots", "node",
                     "--loconet-tcp", str(throttle), "--loconet-tcp-slots", "node"]
        self.start(f"node-{district}", args, stdin=True)

    def joined(self, district):
        """Whether the district's node joined its simulator's DCC-EX and LocoNet buffer."""
        log = self.log(f"sim-{district}")
        return wait_for(log, r"DCC-EX client .* connected", 15) and wait_for(log, r"LocoNet client .* connected", 15)

    def dispatcher(self):
        self.start("dispatcher", ["--mode", "dispatcher", "--session", self.session, "--plain"], stdin=True)

    def place(self, loco, block):
        subprocess.run([BINARY, "--place-train", str(loco), "--block", block, "--session", self.session],
                       stdout=subprocess.DEVNULL, check=True)

    def throttle(self, district):
        return Throttle(DISTRICTS[district][2])

    def status(self, district, loco):
        """The simulator's last status line of a loco: piece, mm along, decoder km/h."""
        lines = re.findall(rf"loco {loco} at (\S+) (\d+) mm, decoder (\d+) km/h", read(self.log(f"sim-{district}")))
        return lines[-1] if lines else None


def run(name, scenario):
    """Runs a scenario on a fresh bench and exits with its result."""
    bench = Bench(name)
    try:
        scenario(bench)
    except Exception:
        traceback.print_exc()
        bench.check("the test itself runs without an error", False)
    finally:
        for process in reversed(bench.processes):
            process.terminate()
        for process in bench.processes:
            process.wait(5)
        print(f"logs in {bench.logs}")
        sys.exit(1 if not bench.checks or any(not ok for _, ok in bench.checks) else 0)
