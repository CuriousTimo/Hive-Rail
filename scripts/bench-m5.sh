#!/usr/bin/env bash
# The bench-m5 test layout on this computer: three districts and six trains, live in the
# web view at http://localhost:8080.
#
#   MacBook   node on this computer, lite server, shares the layout with lite nodes
#   Windows   node on this computer (stands in for a second computer), lite server
#   M5StickC  the Node-Lite on the M5StickC (firmware/node-lite), if it is on the Wi-Fi;
#             it finds a lite server by scouting and fetches its part of the layout
#
# The layout simulator (examples/layout/bench-m5.json, modules drawn from tiles) runs the trains and joins the mesh:
# district power and the fast clock come from the nodes, the trains go into the loco
# register, and each block's occupancy goes to its district's node.
#
# Usage: scripts/bench-m5.sh [--ib-switch]
#   --ib-switch  the MacBook node joins the LocoNet of a GCA101 (UDP multicast) as its only
#                command station and shows eight blocks on an IB-Switch's LEDs (green free,
#                red occupied): key pairs 1-3 Adorf tracks 1-3, 4 the border block to the
#                M5StickC, 11/12 Cdorf eastbound through track and siding, 18/19 westbound.
#
# While it runs, keys go to a node the way its dashboard reads them, one per line:
#   echo 1 > "$RUN/MacBook.keys"   district power on   (0 off, h emergency stop, s short)
#   echo E > "$RUN/Windows.keys"   global emergency stop (G global go)
#   echo c > "$RUN/MacBook.keys"   fast clock run/pause (+ - rate, [ ] hour)
# and to the M5StickC as faked hardware:
#   target/debug/hive-rail --sim-hw M5StickC short --session bench
#   (power_on, power_off, short, global_stop, global_go)
# Watch with the dispatcher in another terminal:
#   target/debug/hive-rail --mode dispatcher --session bench
# Logs are in $RUN (printed at the start). Ctrl+C stops everything.
#
# Settings (environment): SESSION (bench), PORT (8080), RUN (a new temporary directory).
set -euo pipefail

cd "$(dirname "$0")/.."

SESSION=${SESSION:-bench}
PORT=${PORT:-8080}
RUN=${RUN:-$(mktemp -d "${TMPDIR:-/tmp}/hive-rail-bench.XXXXXX")}
LAYOUT=examples/layout/bench-m5.json
HIVE=target/debug/hive-rail
IB_SWITCH=0
[[ "${1:-}" == "--ib-switch" ]] && IB_SWITCH=1

cargo build --quiet
mkdir -p "$RUN"

pids=()
keepers=()
stop() {
	trap - INT TERM EXIT
	echo
	echo "Stopping the bench..."
	kill "${pids[@]}" 2>/dev/null || true
	kill "${keepers[@]}" 2>/dev/null || true
	wait 2>/dev/null || true
	echo "Stopped. Logs: $RUN"
}
trap 'stop; exit 0' INT TERM
trap stop EXIT

# A node reading its keys from a named pipe, kept open so it never sees the end of input.
start_node() {
	local id=$1
	shift
	rm -f "$RUN/$id.keys"
	mkfifo "$RUN/$id.keys"
	sleep 2147483647 >"$RUN/$id.keys" &
	keepers+=($!)
	"$HIVE" --mode node --session "$SESSION" --id "$id" --plain --lite-server --layout "$LAYOUT" "$@" \
		<"$RUN/$id.keys" >"$RUN/$id.log" 2>&1 &
	pids+=($!)
}

macbook_options=(--rocnet-address 101)
if ((IB_SWITCH)); then
	macbook_options+=(--loconet-udp --loconet-connect-slots node
		--show-sensors 111=203,112=205,137=204,131=207,121=221,122=222,126=223,127=224)
fi
start_node MacBook "${macbook_options[@]}"
start_node Windows --rocnet-address 102 --listen tcp/0.0.0.0:7448
sleep 6

"$HIVE" --web-sim "$LAYOUT" --port "$PORT" --session "$SESSION" >"$RUN/web-sim.log" 2>&1 &
pids+=($!)
sleep 4

# Power on in every district (a node starts with its district off) and start the clock.
echo 1 >"$RUN/MacBook.keys"
echo 1 >"$RUN/Windows.keys"
"$HIVE" --sim-hw M5StickC power_on --session "$SESSION" >/dev/null 2>&1 || true
sleep 1
echo c >"$RUN/MacBook.keys"

echo "Hive-Rail bench running (session $SESSION)."
echo "  Web view:   http://localhost:$PORT"
echo "  Dispatcher: $HIVE --mode dispatcher --session $SESSION"
echo "  Keys:       echo h > $RUN/MacBook.keys   (see the top of this script)"
echo "  Logs:       $RUN"
((IB_SWITCH)) && echo "  IB-Switch:  the MacBook node is the command station of the GCA101's LocoNet"
echo "Ctrl+C stops everything."

# Until Ctrl+C, or until a program stops on its own.
while true; do
	for pid in "${pids[@]}"; do
		if ! kill -0 "$pid" 2>/dev/null; then
			echo "A program stopped on its own; see the logs in $RUN"
			exit 1
		fi
	done
	sleep 2
done
