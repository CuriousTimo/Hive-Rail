#!/usr/bin/env bash
# Builds DCC-EX with the Hive-Rail add-on for the node board, and optionally flashes it.
#
#   firmware/node-board/build.sh                      # build only
#   firmware/node-board/build.sh --upload [port]      # build and flash over USB
#
# DCC-EX is an unmodified checkout of https://github.com/DCC-EX/CommandStation-EX next to
# this repository (override with DCCEX_DIR). This script copies the add-on (config.h,
# my*.h, my*.cpp) into it and builds the PlatformIO environment ESP32.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
dccex="${DCCEX_DIR:-$here/../../../CommandStation-EX}"
pio="${PIO:-$(command -v pio || python3 -c 'import os, site; print(os.path.join(site.USER_BASE, "bin", "pio"))')}"

if [[ ! -f "$dccex/CommandStation-EX.ino" ]]; then
	echo "DCC-EX checkout not found at $dccex; clone it or set DCCEX_DIR" >&2
	exit 1
fi

# Every add-on file next to this script: config.h, my*.h and my*.cpp.
addon=("$here"/config.h "$here"/my*.h "$here"/my*.cpp)
cp "${addon[@]}" "$dccex/"
# Keep the copied add-on out of DCC-EX's git status (config.h and my*.h are ignored already).
for file in "${addon[@]}"; do
	name="$(basename "$file")"
	[[ "$name" == *.cpp ]] || continue
	grep -qx "$name" "$dccex/.git/info/exclude" 2>/dev/null ||
		echo "$name" >> "$dccex/.git/info/exclude"
done

echo "== DCC-EX $(git -C "$dccex" describe --tags --always) with the Hive-Rail add-on"
args=(run -d "$dccex" -e ESP32)
if [[ "${1:-}" == "--upload" ]]; then
	args+=(-t upload)
	[[ -n "${2:-}" ]] && args+=(--upload-port "$2")
fi
"$pio" "${args[@]}"
