#!/bin/bash
# usage: shot.sh <name> <delay> <app args...>
OUT=${OUT:-/tmp/tunebox-shots}; mkdir -p "$OUT"
name=$1; delay=$2; shift 2
rm -f $OUT/$name.png
env -u WAYLAND_DISPLAY RUST_LOG=${RUST_LOG:-warn} timeout 45 "${BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/tunebox}" "$@" --screenshot $OUT/$name.png --shot-delay $delay 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | cut -c1-200 | tail -8
ls $OUT/$name.png 2>&1 | tail -1
