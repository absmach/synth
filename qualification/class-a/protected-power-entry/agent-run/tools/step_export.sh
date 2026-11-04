#!/usr/bin/env bash
# step_export.sh <board.kicad_pcb> <out.step>
# Exports the board's STEP model with KiCad's own CLI (the user flatpak, whose sandbox mounts the stock 3D library at
# /app/extensions/Library/3dmodels). The exported board names its models under /usr/share/kicad/3dmodels, so a scratch
# copy with that prefix pointed at the sandbox path is what kicad-cli reads; the board itself is left as it is.
# KiCad's stock library has no model for TestPoint_Pad_D1.5mm, MountingHole_2.5mm, Fuse_1812_4532Metric or the
# SOIC-8-1EP with a 2.41 x 3.3 mm pad: kicad-cli reports each as "File not found" and leaves it out of the STEP.
set -euo pipefail
pcb=$1; out=$2
scratch=$(mktemp -d)
sed 's#/usr/share/kicad/3dmodels#/app/extensions/Library/3dmodels#g' "$pcb" > "$scratch/board.kicad_pcb"
kicad-cli pcb export step --no-dnp -o "$scratch/board.step" "$scratch/board.kicad_pcb" 2>&1 | sort | uniq -c | sort -rn
cp "$scratch/board.step" "$out"
