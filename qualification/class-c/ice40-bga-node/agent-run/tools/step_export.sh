#!/usr/bin/env bash
# step_export.sh <board.kicad_pcb> <out.step>
# Exports the board's STEP model with KiCad's own CLI (the user flatpak, whose sandbox mounts the stock 3D library at
# /app/extensions/Library/3dmodels). It reads a scratch copy of the board, not the board itself, in which:
#   - the exported model prefix /usr/share/kicad/3dmodels is pointed at the sandbox's library;
#   - the USB-C receptacle names the model KiCad's own royalblue54L_feather demo ships for this footprint
#     (the stock 3D library has none for it);
#   - TestPoint_Pad_D1.5mm, which KiCad's library does not have, names the model of ../step-models/ (written by
#     step_models.py: a 1 oz copper disc of the pad's diameter).
# A mounting hole has no body: its hole is drilled in the board solid. step_verify.py checks the result.
set -euo pipefail
pcb=$1; out=$2
here=$(cd "$(dirname "$0")" && pwd)
models=$(cd "$here/../step-models" && pwd)
scratch=$(mktemp -d)
sed -e "s#\${KICAD10_3DMODEL_DIR}/TestPoint.3dshapes/#$models/#" \
    -e 's#/usr/share/kicad/3dmodels/Connector_USB.3dshapes/USB_C_Receptacle_HRO_TYPE-C-31-M-12.step#/app/share/kicad/demos/royalblue54L_feather/lib/3dmodels/Connector_USB.3dshapes/USB_C_Receptacle_HRO_TYPE-C-31-M-12.STEP#' \
    -e 's#/usr/share/kicad/3dmodels#/app/extensions/Library/3dmodels#g' \
    "$pcb" > "$scratch/board.kicad_pcb"
kicad-cli pcb export step --no-dnp -o "$scratch/board.step" "$scratch/board.kicad_pcb" 2>&1 | sort | uniq -c | sort -rn
cp "$scratch/board.step" "$out"
