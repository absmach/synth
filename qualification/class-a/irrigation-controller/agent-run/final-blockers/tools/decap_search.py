#!/usr/bin/env python3
"""decap_search.py <workspace> <binary> <cap_w> <cap_h> <workers>   (env TARGETS='C_VDD2:U2.4,...' MAXMM=4)

Moves each named decoupling capacitor to within max_mm (default 4) of its supply pin, one at a time: candidate spots on
rings round the pin are exported in parallel, and the nearest spot whose export releases is kept before the next
capacitor is tried. Reuses the server, export and scoring helpers of release_search_pin.py."""
import re, sys, math
src = open("/home/synth/runs/release_search_pin.py").read()
head = src[:src.index("main = Server(ws)")]
exec(head)
import os
import os
targets = [t.split(":") for t in os.environ["TARGETS"].split(",")]
MAXMM = float(os.environ.get("MAXMM", "4.0"))
ns = {}
ns = {}
exec(open("/home/synth/who.py").read().split("pcb=parse")[0], ns)
sparse, sfind = ns["parse"], ns["find"]
sparse, sfind = ns["parse"], ns["find"]
def pin_positions(pcbfile):
    p = sparse(open(pcbfile).read()); out = {}
    for fp in sfind(p, "footprint"):
        at = sfind(fp, "at")[0]; fx, fy = float(at[1]), float(at[2]); rot = float(at[3]) if len(at) > 3 else 0
        ref = [q[2] for q in sfind(fp, "property") if q[1] == "Reference"][0]
        c, s = math.cos(math.radians(-rot)), math.sin(math.radians(-rot))
        for pad in sfind(fp, "pad"):
            pa = sfind(pad, "at")[0]; x, y = float(pa[1]), float(pa[2])
            out[f"{ref}.{pad[1]}"] = (fx + x*c - y*s, fy + x*s + y*c)
    return out
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
    with lock: count[0] += 1; return f"ds_{count[0]:03d}"
base_text = open(side).read()
t0 = tag(); base_score, base_blocks, base_pcb = with_server(lambda s: trial(s, base_text, {}, t0))
log("base", base_score, [b[:90] for b in base_blocks])
log("base score", base_score, "(moves are kept when they do not raise it)")
for cap, pinref in targets:
    out, parts = read_pcb(base_pcb); pins = pin_positions(base_pcb)
    px, py = pins[pinref]; cb = parts[cap]; w, h = cb[2]-cb[0], cb[3]-cb[1]
    cx, cy = (cb[0]+cb[2])/2, (cb[1]+cb[3])/2
    cur = math.hypot(cx-px, cy-py)
    log(cap, "now", round(cur, 1), "mm from", pinref)
    if cur <= MAXMM: continue
    cands = []
    SWAPPABLE = ("C", "R", "D", "TP")
    def hits(rect, skip):
        m = 0.25
        return [ref for ref, bb in parts.items() if ref not in skip and not (rect[2]+m <= bb[0] or bb[2]+m <= rect[0] or rect[3]+m <= bb[1] or bb[3]+m <= rect[1])]
    for r in [float(v) for v in os.environ.get('RINGS', '1.8,2.4,3.0,3.6').split(',')]:
        for k in range(16):
            a = k*math.pi/8; nx, ny = px + r*math.cos(a), py + r*math.sin(a)
            rect = (nx-w/2, ny-h/2, nx+w/2, ny+h/2)
            if rect[0] < out[0]+RIM or rect[1] < out[1]+RIM or rect[2] > out[2]-RIM or rect[3] > out[3]-RIM: continue
            hit = hits(rect, {cap})
            if not hit: cands.append((r, round(nx, 2), round(ny, 2), None)); continue
            if len(hit) == 1 and hit[0].startswith(SWAPPABLE) and not hit[0].startswith("TP_"):
                x = hit[0]; xb = parts[x]; xw, xh = xb[2]-xb[0], xb[3]-xb[1]
                xrect = (cx-xw/2, cy-xh/2, cx+xw/2, cy+xh/2)
                if xrect[0] < out[0]+RIM or xrect[1] < out[1]+RIM or xrect[2] > out[2]-RIM or xrect[3] > out[3]-RIM: continue
                # the swapped part lands where the capacitor was, clear of everything but the capacitor's old and new boxes
                others = hits(xrect, {cap, x})
                if others: continue
                cands.append((r + 0.5, round(nx, 2), round(ny, 2), (x, round(cx, 2), round(cy, 2))))
    cands.sort(key=lambda c: (c[0], c[1], c[2])); done = False
    log(cap, len(cands), 'candidate spots (direct or by swap)')
    for batch in range(0, min(len(cands), 3*WORKERS), WORKERS):
        group = cands[batch:batch+WORKERS]
        with cf.ThreadPoolExecutor(WORKERS) as ex:
            futs = {ex.submit(lambda c=c: with_server(lambda s: trial(s, base_text, ({cap: (c[1], c[2])} | ({c[3][0]: (c[3][1], c[3][2])} if c[3] else {})), tag()))): c for c in group}
            res = [(f.result(), futs[f]) for f in futs]
        for (sc, bl, pc), c in sorted(res, key=lambda rc: (rc[0][0], rc[1][0])):
            log(cap, c[:3], "swap" if c[3] else "", "score", sc, [b[:70] for b in bl][:2])
        good = [(r, c) for r, c in res if r[0] <= base_score and r[2]]
        if good:
            (sc, bl, pc), c = sorted(good, key=lambda rc: rc[1][0])[0]
            base_text = set_pos(base_text, cap, c[1], c[2], None); base_score = sc
            if c[3]: base_text = set_pos(base_text, c[3][0], c[3][1], c[3][2], None)
            open(side, "w").write(base_text); base_pcb = pc
            log("KEPT", cap, "at", c[1], c[2], "ring", c[0], "pcb", pc); done = True; break
    if not done: log("FAILED", cap, "no released spot within", MAXMM, "mm")
log("DONE", "sidecar", side)
