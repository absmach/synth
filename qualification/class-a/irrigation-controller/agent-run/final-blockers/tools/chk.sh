d=$1
python3 - <<PY
import json,re
d="$d"
r=json.load(open(d+"/irrigation_controller.routing.json"));v=r["validation"]
print(r["state"],"nets",r["statistics"]["connected_nets"],"open",r["statistics"]["open_nets"],"vias",r["statistics"]["vias"],"fab_ready",v["fabrication_ready"],"blocking",len(v["blocking_reasons"]))
for b in v["blocking_reasons"][:6]: print(" ",b[:130])
s=open(d+"/irrigation_controller.kicad_pcb").read()
ex=[];ey=[]
for m in re.finditer(r"\(gr_line\s*\(start ([-\d.]+) ([-\d.]+)\)\s*\(end ([-\d.]+) ([-\d.]+)\)[\s\S]{0,250}?Edge\.Cuts",s):
    ex+=[float(m.group(1)),float(m.group(3))];ey+=[float(m.group(2)),float(m.group(4))]
print("outline",round(max(ex)-min(ex),1) if ex else None,"x",round(max(ey)-min(ey),1) if ey else None)
PY
