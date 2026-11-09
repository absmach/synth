#!/usr/bin/env python3
"""set_pos.py <sidecar.toml> <out.toml> REF=x,y [REF=x,y ...]: change the courtyard centres of the named parts in a sidecar."""
import re, sys
side, out, *moves = sys.argv[1:]
t = open(side).read()
for mv in moves:
    ref, xy = mv.split("=")
    x, y = xy.split(",")
    pat = re.compile(r"(\[components\.%s\]\nx = )[-\d.]+(\ny = )[-\d.]+" % re.escape(ref))
    assert pat.search(t), ref
    t = pat.sub(lambda m: m.group(1) + x + m.group(2) + y, t, count=1)
open(out, "w").write(t)
print("set", len(moves), "positions")
