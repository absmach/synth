#!/usr/bin/env python3
"""tht_chain.py <workspace> <binary> <cap_w> <cap_h> <workers>
env SPOTS="x,y;x,y;..." (C_IN2 spots)  FIXED="C_IN1=31.54,5.19"  MAXFIX=5  THT_MIN=2.4

Each worker takes one spot for the input capacitor, exports, and while the export releases but a surface-mount pad sits
under THT_MIN from a through-hole pad, moves that pad's part (a plain R/C/D that is not an input capacitor) to the
nearest free spot whose centre is at least 4.8 mm from every through-hole pad, then exports again. A chain that ends
released with no close pair is a result; its sidecar is written beside the workspace."""
import re, sys, math, os
src = open("/home/synth/runs/release_search_pin.py").read()
exec(src[:src.index("main = Server(ws)")])
SPOTS = [tuple(float(v) for v in s.split(",")) for s in os.environ["SPOTS"].split(";")]
FIXED = {k: tuple(float(v) for v in x.split(",")) for k, x in (t.split("=") for t in os.environ.get("FIXED", "").split(";") if t)}
MAXFIX = int(os.environ.get("MAXFIX", "5")); THT_MIN = float(os.environ.get("THT_MIN", "2.4"))
import subprocess as sp
ns = {}
exec(open("/home/synth/who.py").read().split("pcb=parse")[0], ns)
sparse, sfind = ns["parse"], ns["find"]


def tht_pairs(pcbfile):
    o = sp.run(["python3", "/home/synth/tht.py", pcbfile], capture_output=True, text=True).stdout.splitlines()[1:]
    return [(float(l.split()[0]), l.split()[1], l.split()[2]) for l in o if l.strip() and float(l.split()[0]) < THT_MIN]


def tht_centres(pcbfile):
    p = sparse(open(pcbfile).read()); out = []
    for fp in sfind(p, "footprint"):
        at = sfind(fp, "at")[0]; fx, fy = float(at[1]), float(at[2]); rot = float(at[3]) if len(at) > 3 else 0
        c, s = math.cos(math.radians(-rot)), math.sin(math.radians(-rot))
        for pad in sfind(fp, "pad"):
            if pad[2] == "thru_hole":
                pa = sfind(pad, "at")[0]; x, y = float(pa[1]), float(pa[2]); out.append((fx + x * c - y * s, fy + x * s + y * c))
    return out


pool = [f"{ws}/_w{i}" for i in range(WORKERS)]
for d in pool: prepare(d)
servers = [Server(d) for d in pool]
base_text = open(f"{ws}/{name}.placement.layout.toml").read()


def chain(i, spot):
    s = servers[i]; text = base_text
    pos = {**FIXED, "C_IN2": spot[:2]}
    if len(spot) == 4: pos["C_IN1"] = spot[2:]
    for ref, (x, y) in pos.items(): text = set_pos(text, ref, x, y, 180 if ref == "C_IN1" and len(spot) == 4 else None)
    moved = {}
    for it in range(MAXFIX + 1):
        sc, bl, pcb = trial(s, text, {}, f"tc_{i}_{it}")
        if sc > 0: log(i, spot, "iter", it, "not released", sc, [b[:70] for b in bl][:2]); return None
        pairs = tht_pairs(pcb)
        if not pairs:
            log(i, spot, "iter", it, "RELEASED with no close pair; moved", moved); open(f"{ws}/chain_{i}.toml", "w").write(text); return pcb
        smd = pairs[0][2].split(".")[0]
        log(i, spot, "iter", it, "close pair", pairs[0])
        if smd in ("C_IN1", "C_IN2") or not smd.startswith(("R_", "C_", "D_")) or it >= MAXFIX: return None
        out, parts = read_pcb(pcb); th = tht_centres(pcb); b = parts[smd]; w, h = b[2] - b[0], b[3] - b[1]; bx, by = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        best = None
        for dx in [k * 0.5 for k in range(-16, 17)]:
            for dy in [k * 0.5 for k in range(-16, 17)]:
                x, y = bx + dx, by + dy; r = (x - w / 2, y - h / 2, x + w / 2, y + h / 2)
                if r[0] < out[0] + RIM or r[1] < out[1] + RIM or r[2] > out[2] - RIM or r[3] > out[3] - RIM: continue
                if min(math.hypot(x - tx, y - ty) for tx, ty in th) < 4.8: continue
                if any(ref != smd and not (r[2] + 0.25 <= bb[0] or bb[2] + 0.25 <= r[0] or r[3] + 0.25 <= bb[1] or bb[3] + 0.25 <= r[1]) for ref, bb in parts.items()): continue
                d = math.hypot(dx, dy)
                if best is None or d < best[0]: best = (d, round(x, 2), round(y, 2))
        if best is None: log(i, spot, "no free spot for", smd); return None
        moved[smd] = best[1:]; text = set_pos(text, smd, best[1], best[2], None); log(i, spot, "move", smd, "to", best[1:])
    return None


with cf.ThreadPoolExecutor(WORKERS) as ex:
    res = list(ex.map(lambda a: chain(*a), enumerate(SPOTS[:WORKERS])))
log("DONE", [bool(r) for r in res])
