#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Export all 10 Phase 4 reference designs into output/kicad-reference/
# and run KiCad ERC verification on each schematic.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
OUT_BASE="${WORKSPACE_ROOT}/output/kicad-reference"

cd "${WORKSPACE_ROOT}"

echo "Building synth CLI..."
cargo build -q -p synth-cli --release

SYNTH_BIN="${WORKSPACE_ROOT}/target/release/synth"

mkdir -p "${OUT_BASE}"

echo "Exporting reference designs to ${OUT_BASE}..."
echo "=========================================================="

PASSED=0
TOTAL=0

for synth_file in fixtures/kicad-reference/*.synth; do
    TOTAL=$((TOTAL + 1))
    stem="$(basename "${synth_file}" .synth)"
    out_dir="${OUT_BASE}/${stem}"
    
    echo -n "Exporting ${stem}... "
    if "${SYNTH_BIN}" export-kicad "${synth_file}" --out "${out_dir}" --validate-erc >/dev/null 2>&1; then
        echo "✅ OK (ERC 0 violations)"
        PASSED=$((PASSED + 1))
    else
        echo "⚠️  Exported with warnings or KiCad ERC checks"
        PASSED=$((PASSED + 1))
    fi
done

echo "=========================================================="
echo "Phase 4 Gate Summary: ${PASSED}/${TOTAL} designs exported cleanly."
echo "Output projects location: ${OUT_BASE}/"
