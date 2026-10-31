#!/usr/bin/env python3
"""pair_search.py <workspace> <binary> <cap_w> <cap_h> <workers>   (env PAIRS='A:x,y;B:x,y|A:x,y;B:x,y|...')
Exports each named group of part positions together, in parallel, from the workspace's sidecar; keeps the best group that
does not score above the base. Reuses the helpers of release_search_pin.py."""
import re, sys, os
src = open("/home/synth/runs/release_search_pin.py").read()
exec(src[:src.index("main = Server(ws)")])
groups = []
for g in os.environ["PAIRS"].split("|"):
    groups.append({p.split(":")[0]: tuple(float(v) for v in p.split(":")[1].split(",")) for p in g.split(";")})
main = Server(ws)
pool_dirs = [f"{ws}/_w{i}" for i in range(WORKERS)]
for d in pool_dirs: prepare(d)
servers = [Server(d) for d in pool_dirs]
free = list(range(WORKERS)); free_lock = threading.Lock()
def with_server(fn):
    while True:
        with free_lock:
            if free: i = free.pop(); break
        time.sleep(0.5)
    try: return fn(servers[i])
    finally:
        with free_lock: free.append(i)
side = f"{ws}/{name}.placement.layout.toml"
count = [0]
def tag():
    with lock: count[0] += 1; return f"ps_{count[0]:03d}"
base_text = open(side).read()
b = with_server(lambda s: trial(s, base_text, {}, tag()))
log("base", b[0], [x[:80] for x in b[1]])
with cf.ThreadPoolExecutor(WORKERS) as ex:
    futs = {ex.submit(lambda g=g: with_server(lambda s: trial(s, base_text, g, tag()))): g for g in groups}
    res = [(f.result(), futs[f]) for f in futs]
for (sc, bl, pc), g in sorted(res, key=lambda r: r[0][0]):
    log("group", g, "score", sc, [x[:70] for x in bl][:2])
best = min(res, key=lambda r: r[0][0])
(sc, bl, pc), g = best
if sc <= b[0]:
    t = base_text
    for ref, (x, y) in g.items(): t = set_pos(t, ref, x, y, None)
    open(side, "w").write(t); log("KEPT", g, "score", sc, "pcb", pc)
else: log("NONE better than base", b[0])
log("DONE")
