#!/usr/bin/env bash
# step_export.sh <board.kicad_pcb> <out.step>
# Exports the board's STEP model with kicad-cli from a scratch copy of the board in which the three footprints KiCad's
# stock 3D library has no model for (ADS124S08, RP2350, USB-C receptacle) name the models of ../step-models/ (written by
# step_models.py). The board itself is not touched.
set -euo pipefail
pcb=$1; out=$2
models=$(cd "$(dirname "$0")/../step-models" && pwd)
scratch=$(mktemp -d)
sed -E "s#/usr/share/kicad/3dmodels/(Package_DFN_QFN|Connector_USB)\.3dshapes/#$models/#" "$pcb" > "$scratch/board.kicad_pcb"
kicad-cli pcb export step --no-dnp -o "$scratch/board.step" "$scratch/board.kicad_pcb"
cp "$scratch/board.step" "$out"
