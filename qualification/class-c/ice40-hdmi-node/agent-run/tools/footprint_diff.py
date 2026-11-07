#!/usr/bin/env python3
"""fpdiff.py <board.kicad_pcb> <drc.json> <footprint-library-dir>: for each footprint DRC reports as differing from its library copy, compare the
copper (pads), courtyard and fabrication geometry with the library file, and list what differs."""
import json, re, sys, os, collections
board, drc, libdir = sys.argv[1:4]
def squash(t): return re.sub(r'\((\w+)\s*\n\s*', r'(\1 ', t)
def blocks(t, key):
    t = squash(t)
    out = []; i = 0
    while True:
        i = t.find(key, i)
        if i < 0: return out
        d = 0
        for j in range(i, len(t)):
            if t[j] == '(': d += 1
            elif t[j] == ')':
                d -= 1
                if d == 0: out.append(t[i:j + 1]); i = j + 1; break
def norm(b): return re.sub(r'\s+', ' ', re.sub(r'\(uuid "[^"]*"\)|\(net \d+ "[^"]*"\)|\(net \d+\)|\(tstamp [^)]*\)|\(pintype "[^"]*"\)|\(pinfunction "[^"]*"\)', '', b))
def parts(fp):
    """pads, courtyard and fab geometry, silkscreen geometry as normalized strings"""
    pads = sorted(norm(re.sub(r'\(at ([-\d.]+) ([-\d.]+)( [-\d.]+)?\)', lambda m: f'(at {float(m[1]):.3f} {float(m[2]):.3f})', b)) for b in blocks(fp, '(pad '))
    geo = collections.defaultdict(list)
    for kind in ('fp_line', 'fp_circle', 'fp_arc', 'fp_poly', 'fp_rect', 'fp_text', 'fp_curve'):
        for b in blocks(fp, '(' + kind + ' '):
            m = re.search(r'\(layer "([^"]+)"\)', b)
            geo[m[1] if m else '?'].append(norm(b))
    return pads, {k: sorted(v) for k, v in geo.items()}
t = squash(open(board).read())
fps = blocks(t, '(footprint ')
d = json.load(open(drc)); want = collections.Counter()
for v in d['violations']:
    m = re.search(r"Footprint '([^']+)' does not match copy in library '([^']+)'", v['description'])
    if m: want[(m[2], m[1])] += 1
res = collections.defaultdict(lambda: collections.Counter())
for fp in fps:
    name = re.match(r'\(footprint\s+"([^"]+)"', fp)[1]
    lib, _, nm = name.partition(':')
    if (lib, nm) not in want: continue
    path = os.path.join(libdir, lib + '.pretty', nm + '.kicad_mod')
    if not os.path.exists(path): res[(lib, nm)]['library file missing'] += 1; continue
    lp, lg = parts(open(path).read()); bp, bg = parts(fp)
    diff = []
    if lp != bp: diff.append('pads')
    layers = set(lg) | set(bg)
    for L in sorted(layers):
        if lg.get(L) != bg.get(L): diff.append(L)
    res[(lib, nm)][', '.join(diff) or 'no geometry difference found'] += 1
for (lib, nm), c in sorted(res.items()):
    print(f"{lib}:{nm}  DRC warnings {want[(lib, nm)]}  differs in: {dict(c)}")
