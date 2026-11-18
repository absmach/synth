#!/usr/bin/env python3
"""tht_fix_spot.py <board.kicad_pcb> <sidecar.toml> <out.toml> <ref> [min_gap_mm=2.5]

Moves one part to the nearest free spot where every pad of it is at least min_gap_mm (pad edge to pad edge) from every
through-hole pad, its courtyard is 0.25 mm clear of the others and inside the outline with a 1.35 mm rim. Writes the
sidecar with that part's courtyard centre changed."""
import math, re, sys

pcb, side, out, ref = sys.argv[1:5]
MING = float(sys.argv[5]) if len(sys.argv) > 5 else 2.5
ns = {}
exec(open("/home/synth/who.py").read().split("pcb=parse")[0], ns)
sparse, sfind = ns["parse"], ns["find"]
t = open(pcb).read()
p = sparse(t)


def nums(s):
    return [float(v) for v in re.findall(r"-?\d+\.?\d*", s)]


exs, eys = [], []
for m in re.finditer(r"\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid", t, re.S):
    if "Edge.Cuts" in m.group(4):
        a, b = nums(m.group(2)), nums(m.group(3))
        exs += [a[0], b[0]]
        eys += [a[1], b[1]]
OUT = (min(exs), min(eys), max(exs), max(eys))
RIM = 1.35
parts = {}
for fp in sfind(p, "footprint"):
    at = sfind(fp, "at")[0]
    fx, fy = float(at[1]), float(at[2])
    rot = float(at[3]) if len(at) > 3 else 0
    r_ = [q[2] for q in sfind(fp, "property") if q[1] == "Reference"][0]
    c, s = math.cos(math.radians(-rot)), math.sin(math.radians(-rot))
    pads = []
    for pad in sfind(fp, "pad"):
        pa = sfind(pad, "at")[0]
        x, y = float(pa[1]), float(pa[2])
        sz = sfind(pad, "size")[0]
        w, h = float(sz[1]), float(sz[2])
        if abs(rot) % 180 == 90:
            w, h = h, w
        pads.append((fx + x * c - y * s, fy + x * s + y * c, w / 2, h / 2, pad[2] == "thru_hole"))
    xs, ys = [], []
    for g in sfind(fp, "fp_line") + sfind(fp, "fp_rect"):
        if any(isinstance(q, list) and q[0] == "layer" and q[1] == "F.CrtYd" for q in g):
            for k in ("start", "end"):
                q = sfind(g, k)[0]
                lx, ly = float(q[1]), float(q[2])
                xs.append(fx + lx * c - ly * s)
                ys.append(fy + lx * s + ly * c)
    parts[r_] = dict(pads=pads, cl=(min(xs), min(ys), max(xs), max(ys)) if xs else None)

me = parts[ref]
cl = me["cl"]
cw, ch = cl[2] - cl[0], cl[3] - cl[1]
ccx, ccy = (cl[0] + cl[2]) / 2, (cl[1] + cl[3]) / 2
tht = [pd for r_, d in parts.items() if r_ != ref for pd in d["pads"] if pd[4]]
others = [o["cl"] for r_, o in parts.items() if r_ != ref and o["cl"]]


def pad_gap(a, b):
    dx = max(abs(a[0] - b[0]) - a[2] - b[2], 0)
    dy = max(abs(a[1] - b[1]) - a[3] - b[3], 0)
    return math.hypot(dx, dy)


best = None
for i in range(-160, 161):
    for j in range(-160, 161):
        dx, dy = i * 0.25, j * 0.25
        ncx, ncy = ccx + dx, ccy + dy
        rect = (ncx - cw / 2, ncy - ch / 2, ncx + cw / 2, ncy + ch / 2)
        if rect[0] < OUT[0] + RIM or rect[1] < OUT[1] + RIM or rect[2] > OUT[2] - RIM or rect[3] > OUT[3] - RIM:
            continue
        if any(not (rect[2] + 0.25 <= o[0] or o[2] + 0.25 <= rect[0] or rect[3] + 0.25 <= o[1] or o[3] + 0.25 <= rect[1]) for o in others):
            continue
        if min((pad_gap((pd[0] + dx, pd[1] + dy, pd[2], pd[3], False), th) for pd in me["pads"] for th in tht), default=99) < MING:
            continue
        d = math.hypot(dx, dy)
        if best is None or d < best[0]:
            best = (d, dx, dy)
if best is None:
    sys.exit("no spot")
print(ref, "moves", round(best[1], 2), round(best[2], 2), "mm")
sc = open(side).read()
m = re.search(r"(\[components\.%s\]\nx = )([-\d.]+)(\ny = )([-\d.]+)" % ref, sc)
sc = sc[:m.start()] + m.group(1) + str(round(float(m.group(2)) + best[1], 2)) + m.group(3) + str(round(float(m.group(4)) + best[2], 2)) + sc[m.end():]
open(out, "w").write(sc)
