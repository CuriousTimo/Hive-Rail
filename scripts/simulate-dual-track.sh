#!/usr/bin/env bash
# Simulates four dual-track FREMO modules joined into a loop, each run by its own
# Hive-Rail node, with four locos handed from module to module:
#
#   track 1 (clockwise):         module-1 -> module-2 -> module-3 -> module-4 -> module-1
#   track 2 (counter-clockwise): module-4 -> module-3 -> module-2 -> module-1 -> module-4
#
# Loco 3 and loco 10 run on track 1, loco 23 and loco 1234 on track 2. A small
# LocoNet throttle in this script drives each loco through the LocoNet over TCP port
# of the module it is on. A transfer is make-before-break: the receiving module takes
# the loco (LOCO_ADR, null MOVE_SLOTS, function and speed), and after OVERLAP seconds
# the sending module stops it and frees its slot (SLOT_STAT1). Every node shares its
# locos in its heartbeat, so the dashboard shows each loco moving round the loop.
#
# Faults, by round (one round is DWELL seconds, one transfer per loco):
#   round 2  module-3 power off from its LocoNet (GPOFF, as Rocrail's power button)
#   round 4  module-3 power on again (GPON)
#   round 5  short in module-2 (the node's simulated booster trip, key s on stdin)
#   round 7  module-2 power on again from its LocoNet after the short is cleared
# A train only moves on when both districts have power; otherwise it waits at the
# boundary, which is what the boundary signals are meant to enforce later.
#
# This is a throttle-level simulation: Hive-Rail has no handoff protocol yet, and no
# railway hardware is involved.
#
# Usage: scripts/simulate-dual-track.sh [--headless]
#   Opens an observer dashboard in this terminal (q quits and stops every node).
#   --headless runs LAPS laps (default 2) with log output, then checks the power and
#   short behaviour in the node logs and exits non-zero if a check fails.
#
# Settings (environment): SESSION (fremo-sim), BASE_PORT (12341; modules use
# BASE_PORT..BASE_PORT+3), DWELL seconds per round (8), OVERLAP seconds (2),
# LAPS (0 = run until the dashboard is closed; 2 with --headless).
set -euo pipefail

cd "$(dirname "$0")/.."

SESSION=${SESSION:-fremo-sim}
BASE_PORT=${BASE_PORT:-12341}
DWELL=${DWELL:-8}
OVERLAP=${OVERLAP:-2}
HEADLESS=0
[[ "${1:-}" == "--headless" ]] && HEADLESS=1
if ((HEADLESS)); then
	LAPS=${LAPS:-2}
else
	LAPS=${LAPS:-0}
fi

MODULES=4
# Loco address, track, starting module (1-4), LocoNet speed byte (2-127).
LOCO_ADDRESSES=(3 10 23 1234)
LOCO_TRACKS=(1 1 2 2)
LOCO_MODULES=(1 3 2 4)
LOCO_SPEEDS=(48 64 40 56)
# DIRF: forward, F0 (lights) on.
DIRF=$((0x10))
# District power as the throttle sees it: on, off or short.
POWER=(unused on on on on)

LOG_DIR=$(mktemp -d "${TMPDIR:-/tmp}/hive-rail-sim.XXXXXX")
PIDS=()

cleanup() {
	trap - EXIT INT TERM
	for pid in "${PIDS[@]:-}"; do
		[[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
	done
	wait 2>/dev/null || true
	echo "Simulation stopped; logs are in $LOG_DIR"
}
trap cleanup EXIT INT TERM

log() {
	echo "$(date +%H:%M:%S) $*" >>"$LOG_DIR/throttle.log"
	((HEADLESS)) && echo "$*"
	return 0
}

module_id() { echo "module-$1"; }
module_port() { echo $((BASE_PORT + $1 - 1)); }
module_log() { echo "$LOG_DIR/$(module_id "$1").log"; }

# Next module on a track: track 1 counts up, track 2 counts down.
next_module() {
	local track=$1 module=$2
	if ((track == 1)); then
		echo $((module % MODULES + 1))
	else
		echo $(((module + MODULES - 2) % MODULES + 1))
	fi
}

# Sends one LocoNet message to a module, adding the checksum.
send() {
	local module=$1 checksum=255 hex="" byte
	shift
	for byte in "$@"; do
		checksum=$((checksum ^ byte))
		hex+=$(printf '\\x%02x' "$byte")
	done
	hex+=$(printf '\\x%02x' "$checksum")
	# File descriptors 3 to 6 hold the LocoNet connections to module-1 to module-4.
	printf '%b' "$hex" >&$((module + 2))
}

# Presses a dashboard key on a module node; descriptors 7 to 10 are their stdin.
press() {
	local module=$1 key=$2
	echo "$key" >&$((module + 6))
}

# Each node assigns the lowest free slot, so the throttle can track the slots itself
# without parsing replies. SLOT_<module>_<slot> holds the address in that slot.
take_slot() {
	local module=$1 address=$2 slot
	for ((slot = 1; slot <= 119; slot++)); do
		if [[ -z "$(eval echo "\${SLOT_${module}_${slot}:-}")" ]]; then
			echo "$slot"
			return
		fi
	done
	echo "no free slot on $(module_id "$module")" >&2
	exit 1
}

slot_of() {
	local module=$1 address=$2 slot
	for ((slot = 1; slot <= 119; slot++)); do
		if [[ "$(eval echo "\${SLOT_${module}_${slot}:-}")" == "$address" ]]; then
			echo "$slot"
			return
		fi
	done
}

acquire() {
	local module=$1 address=$2 speed=$3 slot
	slot=$(take_slot "$module" "$address")
	eval "SLOT_${module}_${slot}=$address"
	send "$module" $((0xBF)) $((address >> 7)) $((address & 0x7F))
	send "$module" $((0xBA)) "$slot" "$slot"
	send "$module" $((0xA1)) "$slot" "$DIRF"
	send "$module" $((0xA0)) "$slot" "$speed"
}

release() {
	local module=$1 address=$2 slot
	slot=$(slot_of "$module" "$address")
	send "$module" $((0xA0)) "$slot" 0
	# Status 0x03: free, 128 speed steps.
	send "$module" $((0xB5)) "$slot" $((0x03))
	eval "unset SLOT_${module}_${slot}"
}

power_on() {
	send "$1" $((0x83))
	POWER[$1]=on
	log "FAULT CLEARED: $(module_id "$1") power on from its LocoNet"
}

fault() {
	local round=$1
	case $round in
	2)
		send 3 $((0x82))
		POWER[3]=off
		log "FAULT: $(module_id 3) power off from its LocoNet"
		;;
	4) power_on 3 ;;
	5)
		press 2 s
		POWER[2]=short
		log "FAULT: short in $(module_id 2)"
		;;
	7) power_on 2 ;;
	esac
}

echo "== build"
cargo build --release --quiet
BIN=target/release/hive-rail

echo "== start $MODULES module nodes in session $SESSION (logs in $LOG_DIR)"
for ((module = 1; module <= MODULES; module++)); do
	mkfifo "$LOG_DIR/$(module_id "$module").keys"
	"$BIN" --mode node --session "$SESSION" --id "$(module_id "$module")" \
		--rocnet-address $((200 + module)) --loconet-tcp "$(module_port "$module")" --plain \
		<"$LOG_DIR/$(module_id "$module").keys" >"$(module_log "$module")" 2>&1 &
	PIDS+=($!)
	eval "exec $((module + 6))>\"$LOG_DIR/$(module_id "$module").keys\""
done
for ((module = 1; module <= MODULES; module++)); do
	until grep -q "listening on port" "$(module_log "$module")" 2>/dev/null; do
		sleep 0.2
	done
	eval "exec $((module + 2))<>/dev/tcp/127.0.0.1/$(module_port "$module")"
	# Drain replies and echoes so the node never blocks on a full socket.
	cat <&$((module + 2)) >/dev/null &
	PIDS+=($!)
	send "$module" $((0x83))
done
# Let the nodes see each other before the first power change is reported.
sleep 2

run_trains() {
	local index round=0 from to moved
	for index in "${!LOCO_ADDRESSES[@]}"; do
		acquire "${LOCO_MODULES[$index]}" "${LOCO_ADDRESSES[$index]}" "${LOCO_SPEEDS[$index]}"
		log "loco ${LOCO_ADDRESSES[$index]} starts on track ${LOCO_TRACKS[$index]} at $(module_id "${LOCO_MODULES[$index]}")"
	done
	while ((LAPS == 0 || round < LAPS * MODULES)); do
		sleep "$DWELL"
		round=$((round + 1))
		fault "$round"
		moved=()
		for index in "${!LOCO_ADDRESSES[@]}"; do
			from=${LOCO_MODULES[$index]}
			to=$(next_module "${LOCO_TRACKS[$index]}" "$from")
			if [[ "${POWER[$from]}" != on || "${POWER[$to]}" != on ]]; then
				log "loco ${LOCO_ADDRESSES[$index]} track ${LOCO_TRACKS[$index]}: held at $(module_id "$from") -> $(module_id "$to") ($(module_id "$from") ${POWER[$from]}, $(module_id "$to") ${POWER[$to]})"
				continue
			fi
			acquire "$to" "${LOCO_ADDRESSES[$index]}" "${LOCO_SPEEDS[$index]}"
			moved+=("$index")
			log "loco ${LOCO_ADDRESSES[$index]} track ${LOCO_TRACKS[$index]}: $(module_id "$from") -> $(module_id "$to") entering"
		done
		sleep "$OVERLAP"
		for index in "${moved[@]:-}"; do
			[[ -z "$index" ]] && continue
			from=${LOCO_MODULES[$index]}
			to=$(next_module "${LOCO_TRACKS[$index]}" "$from")
			release "$from" "${LOCO_ADDRESSES[$index]}"
			LOCO_MODULES[$index]=$to
			log "loco ${LOCO_ADDRESSES[$index]} track ${LOCO_TRACKS[$index]}: left $(module_id "$from"), now on $(module_id "$to")"
		done
	done
}

FAILURES=0
check() {
	local description=$1
	shift
	if "$@"; then
		echo "PASS  $description"
	else
		echo "FAIL  $description"
		FAILURES=$((FAILURES + 1))
	fi
}

# Whether a module's log has line $2 and, after it, line $3.
in_order() {
	awk -v first="$2" -v second="$3" 'index($0, first) { seen = 1 } seen && index($0, second) { found = 1 } END { exit !found }' "$(module_log "$1")"
}

# Whether every other module saw module $1's district power go from $2 to $3 and back.
peers_saw() {
	local module=$1 from=$2 to=$3 other
	for ((other = 1; other <= MODULES; other++)); do
		((other == module)) && continue
		in_order "$other" "Peer $(module_id "$module") district power $from -> $to" \
			"Peer $(module_id "$module") district power $to -> ON" || return 1
	done
}

# Whether no loco entered module $1 while its district had no power.
no_entry_without_power() {
	awk -v lost="$2" '
		index($0, lost) { dead = 1 }
		index($0, "EVENT  District power ON") { dead = 0 }
		dead && index($0, "LOCO_ADR") { entered = 1 }
		END { exit entered }' "$(module_log "$1")"
}

# Whether module $1 switched its own district power only once (on at the start).
power_untouched() {
	[[ $(grep -cE "EVENT  District power [A-Z]+$" "$(module_log "$1")") -eq 1 ]]
}

verify() {
	echo "== checks"
	check "module-3 switched off and on from its LocoNet" \
		in_order 3 "EVENT  District power OFF" "EVENT  District power ON"
	check "every other module saw module-3 go OFF and back ON" peers_saw 3 ON OFF
	check "module-2 tripped with a short and was switched on again" \
		in_order 2 "EVENT  SHORT in this district" "EVENT  District power ON"
	check "module-2 told its LocoNet clients power off on the short" \
		in_order 2 "EVENT  SHORT in this district" "-> 82 7D  GPOFF"
	check "every other module saw module-2 go SHORT and back ON" peers_saw 2 ON SHORT
	check "module-1 and module-4 kept power throughout" \
		eval "power_untouched 1 && power_untouched 4"
	check "no loco entered module-3 while it was off" \
		no_entry_without_power 3 "EVENT  District power OFF"
	check "no loco entered module-2 while it was shorted" \
		no_entry_without_power 2 "EVENT  SHORT in this district"
	check "the throttle held trains at the boundary" grep -q "held at" "$LOG_DIR/throttle.log"
}

if ((HEADLESS)); then
	echo "== run $LAPS lap(s): $((LAPS * MODULES)) rounds of one transfer per loco"
	run_trains
	sleep 1
	verify
	((FAILURES == 0))
else
	run_trains &
	PIDS+=($!)
	echo "== observer dashboard (q quits and stops the simulation)"
	"$BIN" --mode node --session "$SESSION" --id observer --rocnet-address 200
fi
