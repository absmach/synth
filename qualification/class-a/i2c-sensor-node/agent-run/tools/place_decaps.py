#!/usr/bin/env python3
"""place_decaps.py <board.kicad_pcb> <sidecar.toml> <out.toml> 'C1:U1.3,C2:U1.3,...'
Moves each named capacitor to the free spot nearest the named supply pin: its pad on that net ends within 3.6 mm of the pin, its courtyard
clear of every other courtyard by 0.25 mm and inside the board outline with a 1.35 mm rim. Only the named capacitors move, one after the
other (each placed capacitor is an obstacle for the next). Positions are written as sidecar courtyard centres."""
import math, re, sys, tomllib
pcb, side, out, spec = sys.argv[1:5]
t = open(pcb).read()
def nums(s): return [float(v) for v in re.findall(r"-?\d+\.?\d*", s)]
ex = ey = None
exs, eys = [], []
for m in re.finditer(r"\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid", t, re.S):
    if "Edge.Cuts" in m.group(4):
        a, b = nums(m.group(2)), nums(m.group(3)); exs += [a[0], b[0]]; eys += [a[1], b[1]]
OUT = (min(exs), min(eys), max(exs), max(eys)); RIM = 1.35
parts = {}
for blk in re.split(r"\n\t\(footprint\n", t)[1:]:
    ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
    at = re.search(r"\(at ([-\d.]+) ([-\d.]+)(?: ([-\d.]+))?\)", blk)
    fx, fy, rot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0)
    xs, ys = [], []
    for g in re.finditer(r"\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid", blk, re.S):
        if "F.CrtYd" in g.group(4):
            for q in (nums(g.group(2)), nums(g.group(3))): xs.append(q[0]); ys.append(q[1])
    pads = []
    for m in re.finditer(r'\(pad "([^"]*)" (?:smd|thru_hole)[^\n]*\n\s*\(at ([-\d.]+) ([-\d.]+)(?: [-\d.]+)?\)(?:.|\n)*?\(net \d+ "([^"]*)"\)', blk):
        pads.append((m.group(1), float(m.group(2)), float(m.group(3)), m.group(4)))
    parts[ref] = dict(fx=fx, fy=fy, rot=rot, cl=(min(xs), min(ys), max(xs), max(ys)) if xs else (0, 0, 0, 0), pads=pads)
def rot_pt(x, y, deg):  # local -> board offset for footprint rotation deg (KiCad, y down)
    c, s = math.cos(math.radians(-deg)), math.sin(math.radians(-deg)); return x * c - y * s, x * s + y * c
def rect_of(ref, centre=None, deg=None):
    p = parts[ref]; deg = p["rot"] if deg is None else deg
    cx_l, cy_l = (p["cl"][0] + p["cl"][2]) / 2, (p["cl"][1] + p["cl"][3]) / 2
    hw, hh = (p["cl"][2] - p["cl"][0]) / 2, (p["cl"][3] - p["cl"][1]) / 2
    if round(deg) % 180 != 0: hw, hh = hh, hw
    if centre is None:
        ox, oy = rot_pt(cx_l, cy_l, deg); centre = (p["fx"] + ox, p["fy"] + oy)
    return (centre[0] - hw, centre[1] - hh, centre[0] + hw, centre[1] + hh), centre
rects = {r: rect_of(r)[0] for r in parts}
def pad_abs(ref, pname_net, centre, deg):
    p = parts[ref]; cx_l, cy_l = (p["cl"][0] + p["cl"][2]) / 2, (p["cl"][1] + p["cl"][3]) / 2
    ox, oy = rot_pt(cx_l, cy_l, deg); fx, fy = centre[0] - ox, centre[1] - oy
    res = []
    for n, x, y, net in p["pads"]:
        dx, dy = rot_pt(x, y, deg); res.append((n, fx + dx, fy + dy, net))
    return res
def pin_pos(spec_pin):
    ic, pn = spec_pin.split(".")
    res = []
    for n, x, y, net in parts[ic]["pads"]:
        if n == pn:
            dx, dy = rot_pt(x, y, parts[ic]["rot"]); res.append((parts[ic]["fx"] + dx, parts[ic]["fy"] + dy)); nn = net
    if not res: raise SystemExit("no pin " + spec_pin)
    return res, nn
def clear(r, skip):
    if r[0] < OUT[0] + RIM or r[1] < OUT[1] + RIM or r[2] > OUT[2] - RIM or r[3] > OUT[3] - RIM: return False
    for k, b in rects.items():
        if k == skip: continue
        if not (r[2] + 0.25 <= b[0] or b[2] + 0.25 <= r[0] or r[3] + 0.25 <= b[1] or b[3] + 0.25 <= r[1]): return False
    return True
new = {}
for item in spec.split(","):
    cap, pin = item.split(":"); PS, net = pin_pos(pin); P = PS[0]
    best = None
    for r_ in [x * 0.3 for x in range(2, 20)]:
        for k in range(24 * len(PS)):
            Q = PS[k % len(PS)]; a = (k // len(PS)) * math.pi / 12; c = (Q[0] + r_ * math.cos(a), Q[1] + r_ * math.sin(a))
            for deg in (0, 90, 180, 270):
                rect, centre = rect_of(cap, c, deg)
                if not clear(rect, cap): continue
                pads = [q for q in pad_abs(cap, None, centre, deg) if q[3] == net]
                if not pads: continue
                d = min(math.hypot(q[1] - Z[0], q[2] - Z[1]) for q in pads for Z in PS)
                if best is None or d < best[0] - 1e-9: best = (d, centre, deg, rect)
        if best and best[0] <= 3.0: break
    if not best: print(cap, "no free spot near", pin); continue
    rects[cap] = best[3]; new[cap] = (round(best[1][0], 2), round(best[1][1], 2), best[2]); print(f"{cap}: pad to {pin} {best[0]:.1f} mm at {new[cap]}")
s = open(side).read()
for cap, (x, y, deg) in new.items():
    pat = re.compile(r"(\[components\.%s\]\nx = )[-\d.]+(\ny = )[-\d.]+(\nrotation = )[-\d.]+" % cap)
    if pat.search(s): s = pat.sub(lambda m: m.group(1) + repr(x) + m.group(2) + repr(y) + m.group(3) + repr(float(deg)), s)
    else: s += f'\n[components.{cap}]\nx = {x}\ny = {y}\nrotation = {float(deg)}\nsource = "agent"\npriority = "hard"\ndx = 0.0\ndy = 0.0\n'
open(out, "w").write(s)
