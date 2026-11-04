#!/usr/bin/env bash
# step_export.sh <board.kicad_pcb> <out.step>
# Exports the board's STEP model with KiCad's own CLI (the user flatpak, whose sandbox mounts the stock 3D library at
# /app/extensions/Library/3dmodels). It reads a scratch copy of the board, not the board itself, in which:
#   - the exported model prefix /usr/share/kicad/3dmodels is pointed at the sandbox's library;
#   - TestPoint_Pad_D1.5mm and Fuse_1812_4532Metric, which KiCad's library does not have, name the envelope models of
#     ../step-models/ (written by step_models.py);
#   - the MP2307's SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm names the library's EP2.41x3.81mm model: the same
#     JEDEC body and leads, with a 0.51 mm longer exposed pad under the body.
# A mounting hole has no body: its hole is drilled in the board solid. step_verify.py checks the result.
set -euo pipefail
pcb=$1; out=$2
here=$(cd "$(dirname "$0")" && pwd)
models=$(cd "$here/../step-models" && pwd)
scratch=$(mktemp -d)
sed -e "s#/usr/share/kicad/3dmodels/Fuse.3dshapes/#$models/#" \
    -e "s#\${KICAD10_3DMODEL_DIR}/TestPoint.3dshapes/#$models/#" \
    -e 's#/usr/share/kicad/3dmodels#/app/extensions/Library/3dmodels#g' \
    -e 's#EP2.41x3.3mm.step#EP2.41x3.81mm.step#' "$pcb" > "$scratch/board.kicad_pcb"
kicad-cli pcb export step --no-dnp -o "$scratch/board.step" "$scratch/board.kicad_pcb" 2>&1 | sort | uniq -c | sort -rn
cp "$scratch/board.step" "$out"
