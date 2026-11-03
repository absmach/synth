#!/bin/bash
# fab_package.sh <export-dir> <board-name> <out-dir> [sourcing.csv]
# Builds the fabrication package from an exported board with KiCad's own CLI (run on the droplet):
#   <name>_gerbers.zip  (Gerber layers + Excellon drill, the way JLCPCB reads them)
#   <name>_cpl.csv      (JLCPCB pick-and-place: Designator,Val,Package,Mid X,Mid Y,Rotation,Layer)
#   <name>_bom_orderable.csv (the exported BOM joined with the sourcing table, when given)
set -e; . ~/env.sh
exp=$1; name=$2; out=$3; src=$4
rm -rf "$out"; mkdir -p "$out/gerbers"
pcb="$exp/$name.kicad_pcb"
# MASK_EXPANSION=<mm> builds the Gerbers from a scratch copy whose pad-to-mask clearance is that value
# (the exported board is left as it is); unset, the exported 0 mm (opening equal to the pad) is used.
if [ -n "$MASK_EXPANSION" ]; then
  scratch=$(mktemp -d); cp "$pcb" "$scratch/$name.kicad_pcb"
  sed -i "s/(pad_to_mask_clearance [0-9.]*)/(pad_to_mask_clearance $MASK_EXPANSION)/" "$scratch/$name.kicad_pcb"
  pcb="$scratch/$name.kicad_pcb"
fi
kicad-cli pcb export gerbers --layers F.Cu,B.Cu,F.Paste,B.Paste,F.SilkS,B.SilkS,F.Mask,B.Mask,Edge.Cuts --subtract-soldermask --check-zones -o "$out/gerbers/" "$pcb" >/dev/null
kicad-cli pcb export drill --format excellon --drill-origin absolute --excellon-units mm --generate-map --map-format pdf -o "$out/gerbers/" "$pcb" >/dev/null
python3 - "$out/gerbers" "$out/${name}_gerbers.zip" <<'PYZ'
import os, sys, zipfile
src, dst = sys.argv[1:3]
with zipfile.ZipFile(dst, "w", zipfile.ZIP_DEFLATED) as z:
    for n in sorted(os.listdir(src)):
        z.write(os.path.join(src, n), n)
print(len(os.listdir(src)), "files zipped")
PYZ
kicad-cli pcb export pos --format csv --units mm --side both --exclude-dnp -o "$out/pos_raw.csv" "$pcb" >/dev/null
python3 - "$out/pos_raw.csv" "$out/${name}_cpl.csv" <<'PY'
import csv, sys
rows = list(csv.DictReader(open(sys.argv[1])))
with open(sys.argv[2], "w", newline="") as f:
    w = csv.writer(f); w.writerow(["Designator", "Val", "Package", "Mid X", "Mid Y", "Rotation", "Layer"])
    for r in rows:
        w.writerow([r["Ref"], r["Val"], r["Package"], f'{float(r["PosX"]):.6f}', f'{float(r["PosY"]):.6f}', f'{float(r["Rot"]):.6f}', "Top" if r["Side"].lower() == "top" else "Bottom"])
print(len(rows), "placements")
PY
rm -f "$out/pos_raw.csv"
[ -n "$src" ] && python3 "$(dirname "$0")/bom_orderable.py" "$exp/bom.csv" "$src" "$out/${name}_bom_orderable.csv"
( cd "$out" && sha256sum "${name}_gerbers.zip" "${name}_cpl.csv" ${src:+"${name}_bom_orderable.csv"} > SHA256SUMS )
ls -la "$out"
