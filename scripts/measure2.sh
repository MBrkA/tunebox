#!/bin/bash
# per-thread CPU: usage: measure2.sh <label> <warmup> <window> <args...>   (X11=1 for XWayland)
label=$1; warm=$2; win=$3; shift 3
BIN=${BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/tunebox}
if [ -n "$X11" ]; then ENVC="env -u WAYLAND_DISPLAY"; else ENVC="env"; fi
$ENVC RUST_LOG=${RUST_LOG:-warn} $BIN "$@" > /tmp/measure-$label.log 2>&1 &
PID=$!
sleep $warm
snap(){ for t in /proc/$PID/task/*; do echo "$(basename $t) $(awk '{print $14+$15}' $t/stat 2>/dev/null) $(cat $t/comm 2>/dev/null)"; done; }
snap > /tmp/snap-a; t0=$(date +%s.%N); sleep $win; snap > /tmp/snap-b; t1=$(date +%s.%N)
python3 - "$label" "$t0" "$t1" <<'PY'
import sys,os
label,t0,t1=sys.argv[1],float(sys.argv[2]),float(sys.argv[3]); dt=t1-t0; hz=os.sysconf('SC_CLK_TCK')
def load(p):
    d={}
    for l in open(p):
        tid,ticks,*comm=l.split(None,2); d[tid]=(int(ticks),(comm[0] if comm else '').strip())
    return d
a,b=load('/tmp/snap-a'),load('/tmp/snap-b')
rows=[(round((b[t][0]-a[t][0])/hz/dt*100,2),b[t][1]) for t in b if t in a]
agg={}
for c,n in rows:
    key='llvmpipe/gpu workers' if n.startswith(('llvmpipe','vulkan','wgpu')) else n
    agg[key]=agg.get(key,0)+c
total=sum(agg.values())
print(f"{label}: total={total:.1f}%  "+"  ".join(f"{n}={c:.1f}%" for n,c in sorted(agg.items(),key=lambda x:-x[1])[:7] if c>0.05))
PY
kill $PID 2>/dev/null; wait $PID 2>/dev/null
