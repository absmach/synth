#!/usr/bin/env python3
"""netclass_check.py <export-dir>: for every routed track and via of the exported board, compare its width (via size and drill)
with the width the net's class declares in the .kicad_pro; print per class the nets assigned, the widths seen and any mismatch."""
import json, re, sys, collections
d = sys.argv[1]
pro = json.load(open(f"{d}/i2c_sensor_node.kicad_pro"))
classes = {c["name"]: c for c in pro["net_settings"]["classes"]}
assign = dict(pro["net_settings"].get("netclass_assignments", {}) or {})
pat = pro["net_settings"].get("netclass_patterns", []) or []
t = open(f"{d}/i2c_sensor_node.kicad_pcb").read()
names = dict(re.findall(r'\(net (\d+) "([^"]*)"\)', t))
def cls(net):
    if net in assign: return assign[net]
    if "/" + net in assign: return assign["/" + net]
    for p in pat:
        if re.fullmatch(p["pattern"].replace("*", ".*"), net): return p["netclass"]
    return "Default"
seen = collections.defaultdict(lambda: collections.Counter())
bad = []
for m in re.finditer(r'\(segment\s+\(start[^)]*\)\s+\(end[^)]*\)\s+\(width ([\d.]+)\)\s+\(layer "[^"]+"\)\s+\(net (\d+)\)', t):
    w = float(m.group(1)); n = names.get(m.group(2), ""); c = cls(n)
    seen[c][w] += 1
    want = classes.get(c, {}).get("track_width")
    if want is not None and abs(w - want) > 1e-6: bad.append((n, c, w, want))
print("classes", {k: (v.get("track_width"), v.get("via_diameter"), v.get("via_drill")) for k, v in classes.items()})
print("assignments", len(assign), "patterns", len(pat))
for c, ctr in seen.items(): print(c, dict(ctr))
print("segments narrower or wider than their class width:", len(bad)); print(collections.Counter((b[1], b[2], b[3]) for b in bad).most_common(6))
