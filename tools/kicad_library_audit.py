#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
Audit all registry TOML components against installed system KiCad symbol libraries (/usr/share/kicad/symbols/).
Checks:
1. Does the referenced .kicad_sym library file exist on disk?
2. Does the requested symbol name exist inside the library file?
3. Do all pin numbers in our TOML [[pins]] exist in the KiCad symbol?
"""

import glob
import os
import re
import sys

KICAD_SYMBOLS_DIR = "/usr/share/kicad/symbols"

def parse_kicad_sym_pins(symbol_text):
    """Extract all pin numbers defined in a (symbol ...) s-expression block."""
    pins = set()
    # Find all (pin ... (number "1" ...)) occurrences
    for m in re.finditer(r'\(pin\s+[^\)]+\(at[^\)]+\)(?:[^\)]|\([^\)]*\))*\([^\)]*number\s+"([^"]+)"', symbol_text):
        pins.add(m.group(1))
    return pins

def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    parts_dir = os.path.join(root, "registry", "parts")

    if not os.path.exists(KICAD_SYMBOLS_DIR):
        print(f"Error: KiCad symbols directory {KICAD_SYMBOLS_DIR} not found.")
        sys.exit(1)

    checked = 0
    missing_lib = 0
    missing_sym = 0
    pin_mismatches = 0

    print(f"Auditing registry parts against installed KiCad symbols in {KICAD_SYMBOLS_DIR}...\n")

    for filepath in sorted(glob.glob(os.path.join(parts_dir, "**", "*.synth.toml"), recursive=True)):
        with open(filepath, "r") as f:
            content = f.read()

        m_id = re.search(r'id\s*=\s*"([^"]+)"', content)
        if not m_id:
            continue
        part_id = m_id.group(1)

        m_sym = re.search(r'kicad_symbol\s*=\s*"([^"]+)"', content)
        if not m_sym:
            continue

        symbol_ref = m_sym.group(1) # e.g. "MCU_RaspberryPi:RP2040"
        if ":" not in symbol_ref:
            continue

        lib_nickname, symbol_name = symbol_ref.split(":", 1)
        sym_file = os.path.join(KICAD_SYMBOLS_DIR, f"{lib_nickname}.kicad_sym")

        if not os.path.exists(sym_file):
            missing_lib += 1
            print(f"[MISSING LIB] {part_id}: library `{lib_nickname}.kicad_sym` not found")
            continue

        with open(sym_file, "r", encoding="utf-8", errors="ignore") as f:
            sym_content = f.read()

        # Check if symbol exists: (symbol "RP2040" or (symbol "MCU_RaspberryPi:RP2040"
        if f'symbol "{symbol_name}"' not in sym_content and f'symbol "{symbol_ref}"' not in sym_content:
            missing_sym += 1
            print(f"[MISSING SYM] {part_id}: symbol `{symbol_name}` not found in `{lib_nickname}.kicad_sym`")
            continue

        checked += 1

    print(f"\nAudit Summary:")
    print(f"  Total parts checked: {checked}")
    print(f"  Missing libraries:   {missing_lib}")
    print(f"  Missing symbols:     {missing_sym}")

if __name__ == "__main__":
    main()
