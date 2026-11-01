"""layout_metrics.py <board.kicad_pcb>: numbers for the 12-section layout brief, read from the exported file."""
import re, sys, math
from collections import Counter, defaultdict

def nums(s): return [float(v) for v in re.findall(r'-?\d+\.?\d*', s)]
t = open(sys.argv[1]).read()
ex, ey = [], []
for m in re.finditer(r'\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid', t, re.S):
    if 'Edge.Cuts' in m.group(4):
        a, b = nums(m.group(2)), nums(m.group(3)); ex += [a[0], b[0]]; ey += [a[1], b[1]]
X0, X1, Y0, Y1 = min(ex), max(ex), min(ey), max(ey); W, H = X1 - X0, Y1 - Y0
parts = {}
for blk in re.split(r'\n\t\(footprint\n', t)[1:]:
    ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
    at = re.search(r'\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)', blk)
    cx, cy, rot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0); r = math.radians(rot)
    xs, ys = [], []
    for g in re.finditer(r'\((fp_line|fp_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid', blk, re.S):
        if 'F.CrtYd' in g.group(4):
            for p in (nums(g.group(2)), nums(g.group(3))):
                xs.append(cx + p[0]*math.cos(r) + p[1]*math.sin(r)); ys.append(cy - p[0]*math.sin(r) + p[1]*math.cos(r))
    nets = set()
    for pm in re.finditer(r'\(pad "[^"]*".*?\(net \d+ "([^"]*)"\)', blk, re.S): nets.add(pm.group(1))
    box = (min(xs), min(ys), max(xs), max(ys)) if xs else (cx, cy, cx, cy)
    parts[ref] = dict(c=(cx, cy), rot=rot, box=box, nets=nets)

def gap(a, b):
    dx = max(a[0] - b[2], b[0] - a[2], 0); dy = max(a[1] - b[3], b[1] - a[3], 0); return math.hypot(dx, dy)
def kind(r): return re.match(r'[A-Za-z]+', r).group(0)
feat = {r for r in parts if kind(r) in ('H', 'MH', 'TP', 'FID')}
body = {r: p for r, p in parts.items() if r not in feat}
area_b = sum((p['box'][2]-p['box'][0])*(p['box'][3]-p['box'][1]) for p in body.values())
print(f"board {W:.1f} x {H:.1f} mm = {W*H:.0f} mm2; {len(parts)} parts ({len(body)} excluding holes/test points)")
print(f"courtyard fill: {100*area_b/(W*H):.1f} % of the board area")
bx0 = min(p['box'][0] for p in body.values()); by0 = min(p['box'][1] for p in body.values())
bx1 = max(p['box'][2] for p in body.values()); by1 = max(p['box'][3] for p in body.values())
print(f"parts' bounding box {bx1-bx0:.1f} x {by1-by0:.1f} mm ({100*(bx1-bx0)*(by1-by0)/(W*H):.0f} % of the board)")
# empty quadrants: fraction of 10x10 mm cells with no courtyard
cells = empty = 0
for i in range(int(W//10)):
    for j in range(int(H//10)):
        c = (X0+i*10, Y0+j*10, X0+i*10+10, Y0+j*10+10); cells += 1
        if all(gap(c, p['box']) > 0 or (p['box'][2] < c[0] or p['box'][0] > c[2] or p['box'][3] < c[1] or p['box'][1] > c[3]) for p in parts.values()): empty += 1
print(f"empty 10x10 mm cells: {empty} of {cells} ({100*empty/cells:.0f} %)")
nn = {r: min(gap(p['box'], q['box']) for s, q in body.items() if s != r) for r, p in body.items()}
iso = sorted(((g, r) for r, g in nn.items() if g > 4.0), reverse=True)
print(f"mean nearest-neighbour gap {sum(nn.values())/len(nn):.2f} mm; isolated parts (>4 mm from any body): {[(r, round(g,1)) for g, r in iso]}")
# decoupling: each capacitor to the IC it shares a non-ground net with
print("capacitor -> nearest IC sharing a non-ground net (centre distance, mm):")
ics = [r for r in body if kind(r) in ('U',)]
rows = []
for r, p in body.items():
    if kind(r) != 'C': continue
    best = None
    for u in ics:
        sh = {n for n in (p['nets'] & body[u]['nets']) if n.upper() not in ('GND', 'AGND', 'PGND')}
        if sh:
            d = math.dist(p['c'], body[u]['c']); best = min(best, (d, u, sorted(sh)[0])) if best else (d, u, sorted(sh)[0])
    if best: rows.append((best[0], r, best[1], best[2]))
for d, r, u, n in sorted(rows, reverse=True)[:8]: print(f"   {r:5s} -> {u:3s} via {n:14s} {d:5.1f}")
print(f"   (mean {sum(x[0] for x in rows)/max(len(rows),1):.1f} over {len(rows)} capacitors)")
# test points: distance to the nearest body part on the same net
print("test points -> nearest part on the same net:")
for r, p in sorted(parts.items()):
    if kind(r) != 'TP': continue
    nets = {n for n in p['nets'] if n.upper() not in ('GND',)} or p['nets']
    c = [(math.dist(p['c'], q['c']), s) for s, q in body.items() if q['nets'] & nets]
    print(f"   {r:9s} {min(c)[0]:5.1f} mm to {min(c)[1]}" if c else f"   {r}: none")
# alignment
for k in ('R', 'C', 'D', 'Q'):
    rots = Counter(round(p['rot']) % 360 for r, p in body.items() if kind(r) == k)
    if rots: print(f"{k} orientations: {dict(rots)}")
for pair in (('J_VALVE1', 'J_VALVE2'), ('Q2', 'Q3')):
    if all(x in parts for x in pair):
        a, b = parts[pair[0]], parts[pair[1]]
        print(f"{pair[0]} / {pair[1]}: dx {abs(a['c'][0]-b['c'][0]):.1f} dy {abs(a['c'][1]-b['c'][1]):.1f} mm, rotation {a['rot']:.0f}/{b['rot']:.0f}")
