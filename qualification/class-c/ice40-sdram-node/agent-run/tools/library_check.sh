#!/usr/bin/env bash
# library_check.sh <export-dir> <name> <out-dir>
# Runs KiCad's DRC and ERC on an exported board with BOTH the footprint and the symbol library tables configured.
# Needs KICAD10_FOOTPRINT_DIR and KICAD10_SYMBOL_DIR set to the KiCad 10.0.7 library directories (these are
# listed to build the tables), and kicad-cli 10.0.7.
# The table URIs name the library directories as kicad-cli sees them. Natively that is the same variable, written
# ${KICAD10_FOOTPRINT_DIR}. For KiCad's flatpak, whose sandbox mounts the libraries elsewhere, set
#   FP_URI_DIR=/app/extensions/Library/footprints  SYM_URI_DIR=/app/extensions/Library/symbols
set -euo pipefail
FP_URI_DIR=${FP_URI_DIR:-'${KICAD10_FOOTPRINT_DIR}'}
SYM_URI_DIR=${SYM_URI_DIR:-'${KICAD10_SYMBOL_DIR}'}
exp=$1; name=$2; out=$3
: "${KICAD10_FOOTPRINT_DIR:?set KICAD10_FOOTPRINT_DIR to the KiCad 10.0.7 footprint libraries}"
: "${KICAD10_SYMBOL_DIR:?set KICAD10_SYMBOL_DIR to the KiCad 10.0.7 symbol libraries}"
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
cp "$exp"/* "$work"/
# fp-lib-table: every footprint library, by KiCad's own environment variable
{
  echo '(fp_lib_table'
  echo '  (version 7)'
  for d in "$KICAD10_FOOTPRINT_DIR"/*.pretty; do
    n=$(basename "$d" .pretty)
    echo "  (lib (name \"$n\")(type \"KiCad\")(uri \"$FP_URI_DIR/$n.pretty\")(options \"\")(descr \"\"))"
  done
  echo ')'
} > "$work/fp-lib-table"
# sym-lib-table: the board's own library first, then every standard symbol library
{
  echo '(sym_lib_table'
  echo '  (version 7)'
  sed -n '/^  (lib/,/^  )/p' "$exp/sym-lib-table"
  for f in "$KICAD10_SYMBOL_DIR"/*.kicad_sym; do
    n=$(basename "$f" .kicad_sym)
    echo "  (lib (name \"$n\")(type \"KiCad\")(uri \"$SYM_URI_DIR/$n.kicad_sym\")(options \"\")(descr \"\"))"
  done
  echo ')'
} > "$work/sym-lib-table"
mkdir -p "$out"
kicad-cli pcb drc --refill-zones --severity-all --format json -o "$out/kicad_drc.json" "$work/$name.kicad_pcb" > "$out/drc.log" 2>&1
kicad-cli sch erc --severity-all --format json -o "$out/kicad_erc.json" "$work/$name.kicad_sch" > "$out/erc.log" 2>&1
cp "$work/fp-lib-table" "$work/sym-lib-table" "$out/"
echo "DRC: $(tail -3 "$out/drc.log" | tr '\n' ' ')"
echo "ERC: $(tail -2 "$out/erc.log" | tr '\n' ' ')"
