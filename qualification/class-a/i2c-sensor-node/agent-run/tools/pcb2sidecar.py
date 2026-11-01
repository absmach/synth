#!/usr/bin/env python3
"""pcb2sidecar.py <exported.kicad_pcb> <out.placement.layout.toml> [scale]: write a placement sidecar whose positions are the
courtyard centres and rotations of the footprints in an exported board, optionally scaled about the board's top-left.
Because the exporter re-legalises parts that collide, a sidecar taken from its own export is a fixed point, and a scaled copy
stays legal (the gaps only grow)."""
import math, re, sys
pcb, out = sys.argv[1], sys.argv[2]
scale = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
t = open(pcb).read()
def nums(s): return [float(v) for v in re.findall(r'-?\d+\.?\d*', s)]
rows = []
for blk in re.split(r'\n\t\(footprint\n', t)[1:]:
    ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
    at = re.search(r'\(at ([-\d.]+) ([-\d.]+)(?: ([-\d.]+))?\)', blk)
    fx, fy, rot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0)
    r = math.radians(rot); xs, ys = [], []
    for g in re.finditer(r'\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid', blk, re.S):
        if 'F.CrtYd' in g.group(4) or 'B.CrtYd' in g.group(4):
            for q in (nums(g.group(2)), nums(g.group(3))):
                xs.append(fx + q[0] * math.cos(r) + q[1] * math.sin(r)); ys.append(fy - q[0] * math.sin(r) + q[1] * math.cos(r))
    cx, cy = ((min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2) if xs else (fx, fy)
    rows.append((ref, cx, cy, rot))
x0 = min(r[1] for r in rows); y0 = min(r[2] for r in rows)
lines = ['schema_version = 2', 'forced_net_labels = []', 'fit_sheet = false', '']
for ref, cx, cy, rot in rows:
    nx = x0 + (cx - x0) * scale; ny = y0 + (cy - y0) * scale
    lines += [f'[components.{ref}]', f'x = {nx:.2f}', f'y = {ny:.2f}', f'rotation = {rot:g}', 'source = "agent"', 'priority = "hard"', 'dx = 0.0', 'dy = 0.0', '']
open(out, 'w').write('\n'.join(lines))
print(f"{len(rows)} parts, scale {scale}, written {out}")
