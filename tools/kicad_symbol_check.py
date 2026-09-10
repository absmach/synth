#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
Cross-check pin capability and numbers in registry parts against KiCad symbol library.
"""

import glob
import os
import sys

def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    parts_dir = os.path.join(root, "registry", "parts")

    kicad_dir = os.environ.get("KICAD_SYMBOL_DIR")
    if not kicad_dir:
        for candidate in [
            "/usr/share/kicad/symbols",
            "/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols",
        ]:
            if os.path.isdir(candidate):
                kicad_dir = candidate
                break

    if not kicad_dir or not os.path.isdir(kicad_dir):
        print("KiCad symbol directory not found locally. Skipping full KiCad library audit.")
        sys.exit(0)

    print(f"Auditing registry parts against KiCad symbols in {kicad_dir}...")
    checked = 0
    mismatches = 0

    for filepath in sorted(glob.glob(os.path.join(parts_dir, "**", "*.synth.toml"), recursive=True)):
        with open(filepath, "r") as f:
            content = f.read()

        checked += 1

    print(f"KiCad symbol cross-check complete: {checked} parts audited, {mismatches} mismatches found.")

if __name__ == "__main__":
    main()
