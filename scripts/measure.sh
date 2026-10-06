#!/bin/bash
# usage: measure.sh <label> <warmup-secs> <window-secs> <app args...>   (env X11=1 forces XWayland)
label=$1; warm=$2; win=$3; shift 3
BIN=${BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/tunebox}
if [ -n "$X11" ]; then ENVC="env -u WAYLAND_DISPLAY"; else ENVC="env"; fi
$ENVC RUST_LOG=tunebox=info $BIN "$@" > /tmp/measure-$label.log 2>&1 &
PID=$!
sleep $warm
read_cpu(){ awk '{print $14+$15}' /proc/$PID/stat; }
HZ=$(getconf CLK_TCK)
a=$(read_cpu); t0=$(date +%s.%N); sleep $win; b=$(read_cpu); t1=$(date +%s.%N)
cpu=$(python3 -c "print(round(($b-$a)/$HZ/($t1-$t0)*100,2))")
rss=$(awk '/VmRSS/{print $2/1024}' /proc/$PID/status)
threads=$(ls /proc/$PID/task | wc -l)
echo "$label: cpu=${cpu}% (avg over ${win}s, 100%=1 core)  rss=${rss%.*}MB  threads=$threads  gpu/renderer: $(grep -o 'Software rasterizer\|Using wgpu' /tmp/measure-$label.log | head -1)"
kill $PID 2>/dev/null; wait $PID 2>/dev/null
