#!/usr/bin/env bash
# SYN-363 acceptance check. Run from the repo root: qualification/class-b/rtd-afe/check.sh
# Exports + autoroutes the board, then proves with KiCad itself (not Synth's gate):
#   1. DRC clean including unconnected items, 2. the .kicad_pcb reopens in pcbnew,
#   3. AGND and DGND each exist and are joined at exactly one tie (R_TIE).
set -euo pipefail
B=$(cd "$(dirname "$0")" && pwd); OUT=${OUT:-$B/kicad}; SYNTH=${SYNTH:-target/debug/synth}
export XDG_DATA_HOME=$(mktemp -d)
rm -rf "$OUT" "$B/reports" "$B/images"
$SYNTH export-kicad "$B/rtd_afe.synth" --out "$OUT" --router free-routing \
  --allow-unverified-parts --validate-erc --pnp --gerbers --drill >/dev/null
PCB=$OUT/rtd_afe.kicad_pcb
mkdir -p "$B/reports" "$B/images"
mv "$OUT"/release.json "$OUT"/rtd_afe.routing.json "$OUT"/rtd_afe.connectivity.json "$B/reports/"
rm -f "$OUT"/rtd_afe.freerouting.* "$OUT"/rtd_afe.synth.kicad_* "$OUT"/rtd_afe.drc.json
kicad-cli pcb drc --severity-all --refill-zones --format json -o "$B/reports/rtd_afe.kicad-drc.json" "$PCB" >/dev/null
kicad-cli sch erc --severity-all --format json -o "$B/reports/rtd_afe.kicad-erc.json" "$OUT/rtd_afe.kicad_sch" >/dev/null
kicad-cli sch export netlist --format kicadsexpr -o "$B/reports/rtd_afe.net" "$OUT/rtd_afe.kicad_sch" >/dev/null
for side in top bottom; do kicad-cli pcb render --side $side --width 1600 --height 1200 -o "$B/images/pcb-3d-$side.png" "$PCB" >/dev/null; done
python3 -I - "$B/reports/rtd_afe.kicad-drc.json" "$PCB" <<'PY'
import json, sys, pcbnew
d = json.load(open(sys.argv[1]))
v = [x for x in d["violations"] if x["severity"] == "error"]
u = d["unconnected_items"]
print(f"DRC errors={len(v)} unconnected={len(u)} warnings={sum(x['severity']=='warning' for x in d['violations'])}")
assert not v and not u, (v[:3], u[:3])
b = pcbnew.LoadBoard(sys.argv[2])  # reopen
nets = b.GetNetsByName()
assert nets.has_key("AGND") and nets.has_key("DGND"), "AGND/DGND missing"
ties = [f for f in b.GetFootprints()
        if {p.GetNetname() for p in f.Pads()} >= {"AGND", "DGND"}]
assert [f.GetReference() for f in ties] == ["R_TIE"], [f.GetReference() for f in ties]
# no copper (track, via or zone) may carry both nets: net is per-item, so only pads can tie
print("reopen ok; AGND/DGND tied only at", ties[0].GetReference())
PY
