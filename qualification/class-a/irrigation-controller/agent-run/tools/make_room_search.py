#!/usr/bin/env python3
"""make_room_search.py <workspace> <binary> <cap_w> <cap_h> <workers>
   env TARGETS='C_IN1:U1.2,C_U5:U5.8,...'  MAXMM=4  RINGS=1.8,2.2,...  MAXBLOCK=3  KEEP=C_VDD1,...

Moves each named decoupling capacitor to a spot within reach of its supply pin. A spot taken by parts that do not
serve the pin's IC is made free first: those parts move to the nearest free spots, and the capacitor and the moved
parts go into one trial export. A part serves the IC when it is a capacitor on the pin's own rail, or when it shares
a signal net (not a rail) with the IC, so those stay; connectors, ICs, the crystal, transistors, inductors, fuses
and test points never move. The nearest spot whose export releases is kept before the next capacitor is tried.
Reuses the server, export and scoring helpers of release_search_pin.py."""
import re, sys, math, os
src = open("/home/synth/runs/release_search_pin.py").read()
head = src[:src.index("main = Server(ws)")]
exec(head)
targets = [t.split(":") for t in os.environ["TARGETS"].split(",")]
MAXMM = float(os.environ.get("MAXMM", "4.0"))
MAXBLOCK = int(os.environ.get("MAXBLOCK", "3"))
KEEP = set(os.environ.get("KEEP", "").split(","))
FAR = float(os.environ.get("FAR", "12.0"))
THT_MIN = float(os.environ.get("THT_MIN", "2.4"))
import subprocess as sp
def tht_ok(pcbfile):
    o = sp.run(["python3", "/home/synth/tht.py", pcbfile], capture_output=True, text=True).stdout.splitlines()
    try: return float(o[1].split()[0]) >= THT_MIN - 1e-9
    except Exception: return False
NEAR_OK = {k: float(v) for k, v in (t.split("=") for t in os.environ.get("NEAR_OK", "").split(",") if t)}  # part=max move mm: stays close to where it is
RINGS = [float(v) for v in os.environ.get("RINGS", "1.8,2.2,2.6,3.0,3.4,3.8,4.2").split(",")]
MOVABLE_PREFIX = ("C_", "R_", "D_")
NEVER_PREFIX = ("D_TVS", "D_QG", "D_FZ")
ns = {}
exec(open("/home/synth/who.py").read().split("pcb=parse")[0], ns)
sparse, sfind = ns["parse"], ns["find"]


def read_nets(pcbfile):
    p = sparse(open(pcbfile).read()); out = {}
    for fp in sfind(p, "footprint"):
        ref = [q[2] for q in sfind(fp, "property") if q[1] == "Reference"][0]
        d = {}
        for pad in sfind(fp, "pad"):
            n = sfind(pad, "net")
            d[pad[1]] = n[0][2] if n and len(n[0]) > 2 else ""
        out[ref] = d
    return out


def pin_positions(pcbfile):
    p = sparse(open(pcbfile).read()); out = {}
    for fp in sfind(p, "footprint"):
        at = sfind(fp, "at")[0]; fx, fy = float(at[1]), float(at[2]); rot = float(at[3]) if len(at) > 3 else 0
        ref = [q[2] for q in sfind(fp, "property") if q[1] == "Reference"][0]
        c, s = math.cos(math.radians(-rot)), math.sin(math.radians(-rot))
        for pad in sfind(fp, "pad"):
            pa = sfind(pad, "at")[0]; x, y = float(pa[1]), float(pa[2])
            out[f"{ref}.{pad[1]}"] = (fx + x * c - y * s, fy + x * s + y * c)
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
    with lock: count[0] += 1; return f"mr_{count[0]:03d}"


base_text = open(side).read()
t0 = tag(); base_score, base_blocks, base_pcb = with_server(lambda s: trial(s, base_text, {}, t0))
log("base", base_score, [b[:90] for b in base_blocks])


def inside(r, out): return not (r[0] < out[0] + RIM or r[1] < out[1] + RIM or r[2] > out[2] - RIM or r[3] > out[3] - RIM)


def hit(rect, parts, skip, m=0.25):
    return [ref for ref, bb in parts.items() if ref not in skip and not (rect[2] + m <= bb[0] or bb[2] + m <= rect[0] or rect[3] + m <= bb[1] or bb[3] + m <= rect[1])]


for cap, pinref in targets:
    out, parts = read_pcb(base_pcb); pins = pin_positions(base_pcb); nets = read_nets(base_pcb)
    px, py = pins[pinref]; ic = pinref.split(".")[0]
    cb = parts[cap]; w, h = cb[2] - cb[0], cb[3] - cb[1]; cx, cy = (cb[0] + cb[2]) / 2, (cb[1] + cb[3]) / 2
    cur = math.hypot(cx - px, cy - py)
    log(cap, "now", round(cur, 1), "mm from", pinref)
    if cur <= MAXMM: continue
    # rails: nets with many pads; the pin's own rail; the IC's signal nets
    pads_on = {}
    for r_, d_ in nets.items():
        for n_ in d_.values(): pads_on[n_] = pads_on.get(n_, 0) + 1
    rails = {n for n, c in pads_on.items() if c >= 12 or n.upper().startswith(("GND", "+"))}
    pin_net = nets[ic].get(pinref.split(".")[1], "")
    ic_signal = {n for n in nets[ic].values() if n and n not in rails}

    def movable(ref):
        if ref == cap or ref in KEEP: return False
        if ref in NEAR_OK: return True
        if not ref.startswith(MOVABLE_PREFIX) or ref.startswith(NEVER_PREFIX): return False
        mine = set(nets.get(ref, {}).values())
        if mine & ic_signal: return False
        return True

    def destination(ref, taken):
        b = parts[ref]; bw, bh = b[2] - b[0], b[3] - b[1]; bx, by = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        best = None
        step = 0.5 if ref in NEAR_OK else 1.0
        span = int(NEAR_OK[ref] / step) + 1 if ref in NEAR_OK else 24
        for dx in [k * step for k in range(-span, span + 1)]:
            for dy in [k * step for k in range(-span, span + 1)]:
                x, y = bx + dx, by + dy; r = (x - bw / 2, y - bh / 2, x + bw / 2, y + bh / 2)
                if not inside(r, out): continue
                if ref in NEAR_OK:
                    if math.hypot(dx, dy) > NEAR_OK[ref] or math.hypot(x - px, y - py) < 2.0: continue
                elif math.hypot(x - px, y - py) < FAR: continue
                if hit(r, {k: v for k, v in parts.items() if k not in taken_skip(ref)}, set()) or any(not (r[2] + 0.25 <= t[0] or t[2] + 0.25 <= r[0] or r[3] + 0.25 <= t[1] or t[3] + 0.25 <= r[1]) for t in taken.values()): continue
                dist = math.hypot(dx, dy)
                if best is None or dist < best[0]: best = (dist, round(x, 2), round(y, 2), r)
        return best

    def taken_skip(ref): return {ref, cap}

    cands = []
    seen = set()
    for r_ in RINGS:
        for k in range(24):
            a = k * math.pi / 12; nx, ny = px + r_ * math.cos(a), py + r_ * math.sin(a)
            rect = (nx - w / 2, ny - h / 2, nx + w / 2, ny + h / 2)
            if not inside(rect, out): continue
            blockers = hit(rect, parts, {cap})
            if len(blockers) > MAXBLOCK or any(not movable(b) for b in blockers): continue
            taken = {"cap": rect}; moves = {cap: (round(nx, 2), round(ny, 2))}; total = 0.0; ok = True
            for b in blockers:
                d = destination(b, taken)
                if d is None: ok = False; break
                taken[b] = d[3]; moves[b] = (d[1], d[2]); total += d[0]
            if not ok: continue
            key = tuple(sorted(moves.items()))
            if key in seen: continue
            seen.add(key)
            cands.append((r_, len(blockers), round(total, 1), moves))
    cands.sort(key=lambda c: (c[1], c[0], c[2]))
    log(cap, len(cands), "candidate spots with", MAXBLOCK, "blockers at most")
    done = False
    for batch in range(0, min(len(cands), 4 * WORKERS), WORKERS):
        group = cands[batch:batch + WORKERS]
        with cf.ThreadPoolExecutor(WORKERS) as ex:
            futs = {ex.submit(lambda c=c: with_server(lambda s: trial(s, base_text, c[3], tag()))): c for c in group}
            res = [(f.result(), futs[f]) for f in futs]
        for (sc, bl, pc), c in sorted(res, key=lambda rc: (rc[0][0], rc[1][0])):
            log(cap, "ring", c[0], "blockers", c[1], "moved", {k: v for k, v in c[3].items() if k != cap}, "score", sc, [b[:70] for b in bl][:2])
        good = [(r, c) for r, c in res if r[0] <= base_score and r[2] and tht_ok(r[2])]
        if good:
            (sc, bl, pc), c = sorted(good, key=lambda rc: (rc[1][0], rc[1][1]))[0]
            for ref_, (x_, y_) in c[3].items(): base_text = set_pos(base_text, ref_, x_, y_, None)
            base_score = sc; open(side, "w").write(base_text); base_pcb = pc
            log("KEPT", cap, "ring", c[0], "moved", {k: v for k, v in c[3].items()}, "pcb", pc); done = True; break
    if not done: log("FAILED", cap, "no released spot within", MAXMM, "mm")
log("DONE", "sidecar", side)
