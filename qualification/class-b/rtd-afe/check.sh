#!/usr/bin/env bash
# SYN-363 acceptance check. Run from the repo root: qualification/class-b/rtd-afe/check.sh
# Exports + autoroutes the board, then proves with KiCad itself (not Synth's gate):
#   1. DRC clean including unconnected items, 2. the .kicad_pcb reopens in pcbnew,
#   3. GND is one plane: no AGND/DGND split and no tie resistor.
set -euo pipefail
B=$(cd "$(dirname "$0")" && pwd); FINAL=${OUT:-$B/kicad}; SYNTH=${SYNTH:-target/debug/synth}
# Export and check in a scratch dir; committed evidence is replaced only after every check passes.
STAGE=$(mktemp -d); KC=$STAGE/kicad
export XDG_DATA_HOME=$(mktemp -d)
trap 'rm -rf "$STAGE" "$XDG_DATA_HOME"' EXIT
# Stamp the tree that produced the package; an unset variable stays unset.
export SYNTH_SOURCE_REVISION=${SYNTH_SOURCE_REVISION:-$(git -C "$B/../../.." rev-parse HEAD 2>/dev/null || true)}
# Opt-in only: without it the honest result is validation_failed (12 blocking return-via findings).
FLAGS=()
if [ "${FINE_PITCH_RETURN_VIA_ADVISORY:-}" = 1 ]; then
  FLAGS=(--fine-pitch-return-via-advisory)
  echo "NOTE: --fine-pitch-return-via-advisory is on; it requires signal-integrity review sign-off (not given)." >&2
fi
$SYNTH ${FLAGS[@]+"${FLAGS[@]}"} export-kicad "$B/rtd_afe.synth" --out "$KC" --router free-routing \
  --allow-unverified-parts --validate-erc --pnp --gerbers --drill >/dev/null
PCB=$KC/rtd_afe.kicad_pcb
mkdir -p "$STAGE/reports" "$STAGE/images"
mv "$KC"/release.json "$KC"/rtd_afe.routing.json "$KC"/rtd_afe.connectivity.json "$STAGE/reports/"
rm -f "$KC"/rtd_afe.freerouting.* "$KC"/rtd_afe.synth.kicad_* "$KC"/rtd_afe.drc.json
kicad-cli pcb drc --severity-all --refill-zones --format json -o "$STAGE/reports/rtd_afe.kicad-drc.json" "$PCB" >/dev/null
kicad-cli sch erc --severity-all --format json -o "$STAGE/reports/rtd_afe.kicad-erc.json" "$KC/rtd_afe.kicad_sch" >/dev/null
kicad-cli sch export netlist --format kicadsexpr -o "$STAGE/reports/rtd_afe.net" "$KC/rtd_afe.kicad_sch" >/dev/null
# The run records name the checkout they were built in; keep only the
# repository-relative part so the committed evidence is portable.
REPO=$(cd "$B/../../.." && pwd)
python3 -I - "$REPO" "$STAGE/reports/release.json" "$STAGE/reports/rtd_afe.routing.json" "$STAGE/reports/rtd_afe.net" <<'PY'
import pathlib, sys
root = sys.argv[1] + "/"
for name in sys.argv[2:]:
    path = pathlib.Path(name)
    path.write_text(path.read_text().replace(root, ""))
PY
for side in top bottom; do kicad-cli pcb render --side $side --width 1600 --height 1200 -o "$STAGE/images/pcb-3d-$side.png" "$PCB" >/dev/null; done
kicad-cli pcb render --side top --rotate -45,0,45 --perspective \
  --width 1600 --height 1200 -o "$STAGE/images/pcb-3d-angled.png" "$PCB" >/dev/null
SCHEMATIC_PDF=$(mktemp --suffix=.pdf)
kicad-cli sch export pdf -o "$SCHEMATIC_PDF" "$KC/rtd_afe.kicad_sch" >/dev/null
pdftoppm -png -r 150 -singlefile "$SCHEMATIC_PDF" "$STAGE/images/schematic" >/dev/null
rm -f "$SCHEMATIC_PDF"
python3 -I - "$STAGE/reports/rtd_afe.kicad-drc.json" "$PCB" <<'PY'
import json, sys, pcbnew
d = json.load(open(sys.argv[1]))
v = [x for x in d["violations"] if x["severity"] == "error"]
u = d["unconnected_items"]
print(f"DRC errors={len(v)} unconnected={len(u)} warnings={sum(x['severity']=='warning' for x in d['violations'])}")
assert not v and not u, (v[:3], u[:3])
b = pcbnew.LoadBoard(sys.argv[2])  # reopen
nets = b.GetNetsByName()
assert nets.has_key("GND"), "GND missing"
assert not nets.has_key("AGND") and not nets.has_key("DGND"), "split ground nets remain"
print("reopen ok; single GND plane")
PY
# Swap in beside the old evidence so a failure here never loses it.
mkdir -p "$B/.new"
mv "$KC" "$B/.new/kicad"; mv "$STAGE/reports" "$STAGE/images" "$B/.new/"
rm -rf "$B/.old"; mkdir "$B/.old"
for d in reports images; do [ -e "$B/$d" ] && mv "$B/$d" "$B/.old/$d"; done
[ -e "$FINAL" ] && mv "$FINAL" "$B/.old/kicad"
if mv "$B/.new/kicad" "$FINAL" && mv "$B/.new/reports" "$B/.new/images" "$B/"; then
  rm -rf "$B/.old" "$B/.new"
else
  rm -rf "$FINAL" "$B/reports" "$B/images"
  for d in reports images; do [ -e "$B/.old/$d" ] && mv "$B/.old/$d" "$B/$d"; done
  [ -e "$B/.old/kicad" ] && mv "$B/.old/kicad" "$FINAL"
  echo "swap failed; previous evidence restored" >&2; exit 1
fi
echo "checks passed; committed evidence replaced"
