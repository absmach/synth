#!/usr/bin/env python3
"""valve_path.py <board.kicad_pcb> <net> <source refdes.pin> <loads json>: current in every segment of a power net.

Solves the DC copper network of one net (1 oz copper, 35 um, resistivity 1.72e-8 ohm m; segments are resistors of
rho L / (w t), vias and pads are shorts) from the source pad to the load pads, and prints, for every segment, the
current it carries and the IPC-2221 external temperature rise of its width at that current
(dT = (I / (0.048 A^0.725))^(1 / 0.44), A in square mils). Run with numpy."""
import collections
import json
import math
import re
import sys

import numpy as np

board, net, source, loads = sys.argv[1], sys.argv[2], sys.argv[3], json.loads(sys.argv[4])
t = open(board).read()
nets = {int(a): b for a, b in re.findall(r'\(net (\d+) "([^"]*)"\)', t)}
nid = [k for k, v in nets.items() if v == net][0]
RHO, THICK_M = 1.72e-8, 35e-6
nodes, edges = {}, []


def node(x, y, layer=None):
    key = (round(x, 3), round(y, 3), layer)
    return nodes.setdefault(key, len(nodes))


raw = []
for m in re.finditer(r'\(segment\s+\(start ([\d.-]+) ([\d.-]+)\)\s*\(end ([\d.-]+) ([\d.-]+)\)\s*\(width ([\d.]+)\)\s*\(layer "([^"]+)"\)\s*\(net (\d+)\)', t):
    if int(m.group(7)) == nid:
        x1, y1, x2, y2, w = map(float, m.groups()[:5])
        if (x1, y1) != (x2, y2):
            raw.append([x1, y1, x2, y2, w, m.group(6)])
# A via whose centre lies on the body of a track splits that track there, so the via joins the track at that point.
_vias_early = []
for _b in re.findall(r"\(via\b(.*?)\n\s*\)", t, re.S):
    _n = re.search(r"\(net (\d+)\)", _b)
    _at = re.search(r"\(at ([\d.-]+) ([\d.-]+)\)", _b)
    if _n and int(_n.group(1)) == nid and _at:
        _vias_early.append((float(_at.group(1)), float(_at.group(2))))
for _vx, _vy in _vias_early:
    _k = 0
    while _k < len(raw):
        _x1, _y1, _x2, _y2, _w, _l = raw[_k]
        _dx, _dy = _x2 - _x1, _y2 - _y1
        _u = ((_vx - _x1) * _dx + (_vy - _y1) * _dy) / (_dx * _dx + _dy * _dy)
        if 0.02 < _u < 0.98:
            _px, _py = _x1 + _u * _dx, _y1 + _u * _dy
            if math.hypot(_px - _vx, _py - _vy) <= _w / 2 and math.hypot(_px - _x1, _py - _y1) > 0.02 and math.hypot(_px - _x2, _py - _y2) > 0.02:
                raw[_k] = [_x1, _y1, _px, _py, _w, _l]
                raw.append([_px, _py, _x2, _y2, _w, _l])
                continue
        _k += 1
# Copper that overlaps connects. A track end that lands on the body of another track of the same layer, within half
# the sum of the widths, splits that track there and joins at the split; two ends that overlap join directly.
joins = []
changed = True
while changed:
    changed = False
    for ka, ra in enumerate(raw):
        for end in ((ra[0], ra[1]), (ra[2], ra[3])):
            for kb, rb in enumerate(raw):
                if kb == ka or rb[5] != ra[5]:
                    continue
                x1, y1, x2, y2, wb, layer = rb
                dx, dy = x2 - x1, y2 - y1
                u = ((end[0] - x1) * dx + (end[1] - y1) * dy) / (dx * dx + dy * dy)
                if 0.02 < u < 0.98:
                    px_, py_ = x1 + u * dx, y1 + u * dy
                    if math.hypot(px_ - end[0], py_ - end[1]) <= (ra[4] + wb) / 2 - 0.01 and (round(px_, 3), round(py_, 3)) not in {(round(x1, 3), round(y1, 3)), (round(x2, 3), round(y2, 3))}:
                        raw[kb] = [x1, y1, px_, py_, wb, layer]
                        raw.append([px_, py_, x2, y2, wb, layer])
                        joins.append(((px_, py_, layer), (end[0], end[1], layer)))
                        changed = True
                        break
            if changed:
                break
        if changed:
            break
for k, a in enumerate(raw):
    for b in raw[k + 1:]:
        if a[5] == b[5]:
            for ea in ((a[0], a[1]), (a[2], a[3])):
                for eb in ((b[0], b[1]), (b[2], b[3])):
                    if 0 < math.hypot(ea[0] - eb[0], ea[1] - eb[1]) <= (a[4] + b[4]) / 2 - 0.01:
                        joins.append(((ea[0], ea[1], a[5]), (eb[0], eb[1], b[5])))
for x1, y1, x2, y2, w, layer in raw:
    edges.append((node(x1, y1, layer), node(x2, y2, layer), w, math.hypot(x2 - x1, y2 - y1), layer, (x1, y1, x2, y2)))
vias = []
for b in re.findall(r"\(via\b(.*?)\n\s*\)", t, re.S):
    n = re.search(r"\(net (\d+)\)", b)
    at = re.search(r"\(at ([\d.-]+) ([\d.-]+)\)", b)
    if n and int(n.group(1)) == nid and at:
        size = re.search(r"\(size ([\d.]+)\)", b)
        vias.append((float(at.group(1)), float(at.group(2)), float(size.group(1)) if size else 0.6))
pads = {}
for fp in re.split(r"\n\t\(footprint\s", t)[1:]:
    ref = re.search(r'\(property\s+"Reference"\s+"([^"]+)"', fp).group(1)
    layer_fp = re.search(r'\(layer "([^"]+)"\)', fp).group(1)
    at = re.search(r"\n\t\t\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)", fp)
    fx, fy, frot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0)
    rot = math.radians(frot)
    for p in re.finditer(r'\(pad "([^"]*)" (\w+) (\w+)\s+\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)\s*\(size ([\d.]+) ([\d.]+)\)(.*?)\(net (\d+) "([^"]*)"\)', fp, re.S):
        if p.group(11) == net:
            x, y = float(p.group(4)), float(p.group(5))
            prot = math.radians(frot + float(p.group(6) or 0))
            pads[f"{ref}.{p.group(1)}"] = (fx + x * math.cos(rot) + y * math.sin(rot), fy - x * math.sin(rot) + y * math.cos(rot), p.group(2), float(p.group(7)), float(p.group(8)), prot, layer_fp)
# join: a via or a through-hole pad shorts its layers; any pad shorts to the segment ends within 0.25 mm on its layer
points = collections.defaultdict(list)
for (x, y, layer), idx in nodes.items():
    points[(x, y)].append(idx)
uf = list(range(10 * len(nodes) + 10))


def find(a):
    while uf[a] != a:
        uf[a] = uf[uf[a]]
        a = uf[a]
    return a


def union(a, b):
    uf[find(a)] = find(b)


for (x, y), idxs in points.items():
    pass
keys = list(nodes.items())
for ai in range(len(keys)):
    for bi in range(ai + 1, len(keys)):
        (xa, ya, la), ia = keys[ai]
        (xb, yb, lb), ib = keys[bi]
        if la == lb and math.hypot(xa - xb, ya - yb) < 0.02:
            union(ia, ib)
for ja, jb in joins:
    union(node(*ja), node(*jb))
for (vx, vy, vsize) in vias:
    near = [i for (x, y, l), i in nodes.items() if math.hypot(x - vx, y - vy) <= vsize / 2 + 0.02]
    for i in near[1:]:
        union(near[0], i)
pad_node = {}
for name, (px, py, kind, pw, ph, prot, fpl) in pads.items():
    inside = []
    for a, b, w, length, layer, geo in edges:
        if not (kind == "thru_hole" or layer == fpl):
            continue
        x1, y1, x2, y2 = geo
        steps = max(2, int(length / 0.05) + 1)
        for k in range(steps + 1):
            x, y = x1 + (x2 - x1) * k / steps, y1 + (y2 - y1) * k / steps
            dx, dy = x - px, y - py
            u = dx * math.cos(prot) - dy * math.sin(prot)
            v = dx * math.sin(prot) + dy * math.cos(prot)
            if abs(u) <= pw / 2 + w / 2 and abs(v) <= ph / 2 + w / 2:
                inside.extend([a, b])
                break
    # A via inside the pad (via-in-pad) is the pad's own connection to the other layer, with or without a track end on it.
    for (vx, vy, vsize) in vias:
        dx, dy = vx - px, vy - py
        u = dx * math.cos(prot) - dy * math.sin(prot)
        v = dx * math.sin(prot) + dy * math.cos(prot)
        if abs(u) <= pw / 2 and abs(v) <= ph / 2:
            inside.extend(i for (x, y, l), i in nodes.items() if math.hypot(x - vx, y - vy) <= vsize / 2 + 0.02)
    if inside:
        pad_node[name] = inside[0]
        for i in inside[1:]:
            union(inside[0], i)
missing = [p for p in list(loads) + [source] if p not in pad_node]
if missing:
    print("pads not on copper of the net:", missing)
comp = {}
for i in range(len(nodes)):
    comp.setdefault(find(i), len(comp))
n = len(comp)
G = np.zeros((n, n))
for a, b, w, length, layer, _ in edges:
    r = RHO * (length * 1e-3) / ((w * 1e-3) * THICK_M)
    ca, cb = comp[find(a)], comp[find(b)]
    if ca == cb:
        continue
    g = 1 / r
    G[ca, ca] += g; G[cb, cb] += g; G[ca, cb] -= g; G[cb, ca] -= g
inj = np.zeros(n)
total = 0.0
for p, amps in loads.items():
    if p in pad_node:
        inj[comp[find(pad_node[p])]] -= amps
        total += amps
inj[comp[find(pad_node[source])]] += total
keep = [i for i in range(n) if i != comp[find(pad_node[source])]]
V = np.zeros(n)
adj = collections.defaultdict(set)
for a, b, *_ in edges:
    adj[comp[find(a)]].add(comp[find(b)]); adj[comp[find(b)]].add(comp[find(a)])
seen, stack = set(), [comp[find(pad_node[source])]]
while stack:
    c = stack.pop()
    if c not in seen:
        seen.add(c); stack.extend(adj[c])
unreached = [p for p in loads if p in pad_node and comp[find(pad_node[p])] not in seen]
if unreached:
    print("loads not connected to the source through the net copper:", unreached)
V[keep] = np.linalg.solve(G[np.ix_(keep, keep)] + 1e-6 * np.eye(len(keep)), inj[keep])
rows = []
for a, b, w, length, layer, geo in edges:
    ca, cb = comp[find(a)], comp[find(b)]
    r = RHO * (length * 1e-3) / ((w * 1e-3) * THICK_M)
    i = abs(V[ca] - V[cb]) / r
    area_mil2 = (w / 0.0254) * 1.378
    dt = (i / (0.048 * area_mil2 ** 0.725)) ** (1 / 0.44) if i > 0 else 0.0
    rows.append(dict(width=round(w, 3), length=round(length, 2), layer=layer, current_a=round(i, 3), rise_c=round(dt, 1), at=[round(v, 2) for v in geo]))
narrow = [r for r in rows if r["width"] < 0.8 - 1e-6]
out = dict(
    net=net, source=source, loads=loads, total_a=round(total, 3), segments=len(rows),
    narrowed=len(narrow), narrowed_length_mm=round(sum(r["length"] for r in narrow), 1),
    max_rise_all_c=max(r["rise_c"] for r in rows), max_rise_narrowed_c=max(r["rise_c"] for r in narrow),
    worst_narrowed=sorted(narrow, key=lambda r: -r["rise_c"])[:5],
    worst_current_narrowed=sorted(narrow, key=lambda r: -r["current_a"])[:3],
    by_width={str(w): dict(segments=sum(1 for r in rows if r["width"] == w), length_mm=round(sum(r["length"] for r in rows if r["width"] == w), 1), max_current_a=max(r["current_a"] for r in rows if r["width"] == w), max_rise_c=max(r["rise_c"] for r in rows if r["width"] == w)) for w in sorted({r["width"] for r in rows})},
    all_segments=rows,
)
print(json.dumps(out, indent=1))
