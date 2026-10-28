#!/usr/bin/env bash
# Build release packages locally:
#   dist/packages/hive-rail-macos-arm64            (native, ad-hoc signed)
#   dist/packages/hive-rail-windows-x86_64.exe     (cargo-zigbuild)
#   dist/packages/hive-rail_<version>_{amd64,arm64}.deb  (cargo-zigbuild + cargo-deb)
#
# Requires: rustup targets below, cargo-zigbuild, zig and cargo-deb.
set -euo pipefail

cd "$(dirname "$0")/.."

out=dist/packages
rm -rf "$out"
mkdir -p "$out"

echo "== macOS arm64"
cargo build --locked --release
cp target/release/hive-rail "$out/hive-rail-macos-arm64"
codesign --force --sign - "$out/hive-rail-macos-arm64"

echo "== Windows x86_64"
cargo zigbuild --locked --release --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/hive-rail.exe "$out/hive-rail-windows-x86_64.exe"

for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
	echo "== Linux $target (.deb)"
	cargo zigbuild --locked --release --target "$target"
	cargo deb --no-build --no-strip --target "$target" --output "$out"
done

(cd "$out" && shasum -a 256 * > SHA256SUMS)
ls -l "$out"
