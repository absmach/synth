#!/usr/bin/env python3
"""collect_facts.py <repro-dir> <check-dir> <out.json>: the measured numbers of the final export (out1) for the README."""
import json, re, sys, math, hashlib, subprocess, collections, glob, os
repro, chk, outp = sys.argv[1:4]
o1 = f"{repro}/out1"
f = {}
r = json.load(open(f"{o1}/i2c_sensor_node.routing.json")); v = r["validation"]
f["state"] = r["state"]; f["fabrication_ready"] = v.get("fabrication_ready"); f["release_ready"] = True if r["state"] == "routed" and not v["blocking_reasons"] else False
f["blocking"] = len(v["blocking_reasons"]); f["kicad_drc_in_router"] = v["kicad_drc"]
st = r.get("statistics", {}); f["nets"] = st.get("connected_nets"); f["open_nets"] = st.get("open_nets"); f["vias"] = st.get("vias"); f["segments"] = st.get("segments")
t = open(f"{o1}/i2c_sensor_node.kicad_pcb").read()
def nums(s): return [float(x) for x in re.findall(r"-?\d+\.?\d*", s)]
ex = []; ey = []
for m in re.finditer(r"\((gr_line|gr_rect)\s+\(start ([^)]*)\)\s+\(end ([^)]*)\)(.*?)\(uuid", t, re.S):
    if "Edge.Cuts" in m.group(4):
        a, b = nums(m.group(2)), nums(m.group(3)); ex += [a[0], b[0]]; ey += [a[1], b[1]]
f["outline_w"] = round(max(ex) - min(ex), 2); f["outline_h"] = round(max(ey) - min(ey), 2); f["outline_area"] = round(f["outline_w"] * f["outline_h"])
refs = re.findall(r'"Reference"\s+"([^"]+)"', t); f["parts"] = len(refs)
f["holes"] = {}
for blk in re.split(r"\n\t\(footprint\n", t)[1:]:
    ref = re.search(r'"Reference"\s+"([^"]+)"', blk).group(1)
    if ref.startswith("H"):
        at = re.search(r"\(at ([-\d.]+) ([-\d.]+)", blk); f["holes"][ref] = (float(at.group(1)), float(at.group(2)))
d = json.load(open(f"{chk}/kicad_drc.json")); f["drc"] = dict(collections.Counter(f"{x['severity']}:{x['type']}" for x in d["violations"])); f["drc_unconnected"] = len(d.get("unconnected_items", []))
e = json.load(open(f"{chk}/kicad_erc.json")); f["erc"] = dict(collections.Counter(f"{x['severity']}:{x['type']}" for s in e["sheets"] for x in s["violations"]))
f["gerber_hash"] = open(f"{chk}/fab/GERBER_CONTENT_SHA256").read().strip() if os.path.exists(f"{chk}/fab/GERBER_CONTENT_SHA256") else None
f["compare"] = subprocess.run(["python3", "/tmp/compare_exports.py", repro], capture_output=True, text=True).stdout
raw = []
for n in (1, 2, 3):
    raw.append(hashlib.sha256(open(f"{repro}/out{n}/i2c_sensor_node.kicad_pcb", "rb").read()).hexdigest()[:10])
f["raw_pcb_hashes"] = raw
f["decap"] = subprocess.run(["python3", os.path.expanduser("~/runs/decap_dist.py"), f"{o1}/i2c_sensor_node.kicad_pcb"], capture_output=True, text=True).stdout
f["edge"] = subprocess.run(["python3", os.path.expanduser("~/runs/edge.py"), f"{o1}/i2c_sensor_node.kicad_pcb"], capture_output=True, text=True).stdout
f["netclass"] = subprocess.run(["python3", os.path.expanduser("~/runs/netclass_check.py"), o1], capture_output=True, text=True).stdout
f["metrics"] = subprocess.run(["python3", os.path.expanduser("~/runs/layout_metrics.py"), f"{o1}/i2c_sensor_node.kicad_pcb"], capture_output=True, text=True).stdout
open(outp, "w").write(json.dumps(f, indent=1))
print(json.dumps({k: f[k] for k in f if k not in ("compare", "decap", "edge", "netclass", "metrics")}, indent=1))
