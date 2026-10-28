#!/usr/bin/env bash
# Local replacement for the CI matrix: run the CI checks natively, then
# cross-build release and test binaries for Linux (x86_64, aarch64) and Windows.
#
# Requires: rustup targets below, cargo-zigbuild, and zig (on PATH, or
# via pip3 install --user ziglang, which cargo-zigbuild finds by itself).
#
# Output: dist/<target>/ holds the release binary, the test binary and
# examples/, so tests can be run on the target machine from that folder.
set -euo pipefail

cd "$(dirname "$0")/.."

TARGETS=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu)

echo "== fmt"
cargo fmt --all -- --check
echo "== clippy"
cargo clippy --locked --all-targets --all-features -- -D warnings
echo "== test (native)"
cargo test --locked

for target in "${TARGETS[@]}"; do
	echo "== cross-build $target"
	out="dist/$target"
	rm -rf "$out"
	mkdir -p "$out"
	cargo zigbuild --locked --release --target "$target"
	test_bin=$(cargo zigbuild --locked --tests --target "$target" --message-format=json 2>/dev/null |
		python3 -c 'import sys, json
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m["target"]["name"] == "hive-rail" and m["profile"]["test"]:
        print(m["executable"])')
	suffix=""
	[[ "$target" == *windows* ]] && suffix=".exe"
	cp "target/$target/release/hive-rail$suffix" "$out/"
	cp "$test_bin" "$out/hive-rail-tests$suffix"
	cp -R examples "$out/"
done

echo "== done; copy dist/<target>/ to a target machine and run ./hive-rail-tests there"
