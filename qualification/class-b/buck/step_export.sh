#!/usr/bin/env bash
# step_export.sh <board.kicad_pcb> <out.step>
# Exports the board's STEP model with kicad-cli from a scratch copy in which F1's Fuse_1812_4532Metric, which KiCad's
# stock 3D library lacks, names the model in step-models/ (written by step_models.py). The board itself is not touched.
set -euo pipefail
pcb=$1; out=$2
models=$(cd "$(dirname "$0")/step-models" && pwd)
scratch=$(mktemp -d)
sed "s#/usr/share/kicad/3dmodels/Fuse.3dshapes/#$models/#" "$pcb" > "$scratch/board.kicad_pcb"
kicad-cli pcb export step --no-dnp -o "$scratch/board.step" "$scratch/board.kicad_pcb"
cp "$scratch/board.step" "$out"
