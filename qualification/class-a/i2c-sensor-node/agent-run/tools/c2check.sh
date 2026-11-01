#!/bin/bash
# c2check.sh <export-dir> <out-dir>: DRC and ERC with the footprint library table, the fab package, hashes
set -e; . ~/env.sh
exp=$1; out=$2; name=i2c_sensor_node
rm -rf "$out"; mkdir -p "$out/drc"
cp "$exp"/*.kicad_sch "$exp"/$name.kicad_pcb "$exp"/$name.kicad_pro "$exp"/$name.kicad_sym "$exp"/sym-lib-table "$out/drc/" 2>/dev/null || true
cp ~/drc-e8/fp-lib-table "$out/drc/"
(cd "$out/drc" && kicad-cli pcb drc --refill-zones --severity-all --format json -o ../kicad_drc.json $name.kicad_pcb | tail -2; kicad-cli sch erc --severity-all --format json -o ../kicad_erc.json $name.kicad_sch | tail -2)
bash ~/fabtools/fab_package.sh "$exp" $name "$out/fab" ~/fabtools/sourcing_c2.csv | tail -3
python3 ~/fabtools/gerber_content_hash.py "$out/fab/${name}_gerbers.zip"
python3 - <<PY
import json,collections
d=json.load(open("$out/kicad_drc.json")); print("DRC",dict(collections.Counter((v["severity"],v["type"]) for v in d["violations"])),"unconnected",len(d.get("unconnected_items",[])))
e=json.load(open("$out/kicad_erc.json")); print("ERC",dict(collections.Counter((v["severity"],v["type"]) for s in e["sheets"] for v in s["violations"])))
PY
