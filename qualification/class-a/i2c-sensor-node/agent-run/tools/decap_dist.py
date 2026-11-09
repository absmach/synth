#!/usr/bin/env python3
"""decap_dist.py <board.kicad_pcb>: for each capacitor, the distance (pad centre to pad centre) from its pad on a rail to the nearest IC pad on the same net."""
import re, sys, math
t = open(sys.argv[1]).read()
def blocks():
    for blk in re.split(r"\n\t\(footprint\n", t)[1:]:
        ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
        at = re.search(r"\(at ([-\d.]+) ([-\d.]+)(?: ([-\d.]+))?\)", blk)
        fx, fy, rot = float(at.group(1)), float(at.group(2)), float(at.group(3) or 0)
        c, s = math.cos(math.radians(-rot)), math.sin(math.radians(-rot))
        pads = []
        for m in re.finditer(r'\(pad "([^"]*)" (?:smd|thru_hole)[^\n]*\n\s*\(at ([-\d.]+) ([-\d.]+)(?: [-\d.]+)?\)(?:.|\n)*?\(net \d+ "([^"]*)"\)', blk):
            x, y = float(m.group(2)), float(m.group(3)); pads.append((m.group(1), fx + x*c - y*s, fy + x*s + y*c, m.group(4)))
        yield ref, pads
parts = dict(blocks())
ics = {r: p for r, p in parts.items() if r.startswith(("U", "J"))}
for r in sorted(parts, key=lambda k: (k[0], int(re.sub(r"\D", "", k) or 0))):
    if not r.startswith("C"): continue
    best = None
    for name, x, y, net in parts[r]:
        if net in ("GND", "") or net.startswith("GND"): continue
        for ic, pads in ics.items():
            for pn, px, py, pnet in pads:
                if pnet == net:
                    d = math.hypot(x - px, y - py)
                    if best is None or d < best[0]: best = (d, net, ic, pn)
    if best: print(f"{r:4s} pad on {best[1]:9s} -> {best[2]}.{best[3]:3s} {best[0]:5.1f} mm")
