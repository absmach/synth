#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
enrich_registry_tomls.py — Enrich all registry TOML files in registry/parts/
with [footprint_dimensions] and [operating_conditions] sections, and unit annotations
for multi-gate components.
"""

import os
import re
import sys
from pathlib import Path

REGISTRY_ROOT = Path(__file__).parent.parent / "registry" / "parts"
KICAD_FP_ROOT = Path("/usr/share/kicad/footprints")

def get_fp_dims(fp_ref: str, filename: str) -> tuple[float, float, float]:
    """Extract footprint dimensions (width_mm, height_mm, courtyard_margin_mm)."""
    if fp_ref and ":" in fp_ref:
        lib, fp = fp_ref.split(":", 1)
        mod_path = KICAD_FP_ROOT / f"{lib}.pretty" / f"{fp}.kicad_mod"
        if mod_path.exists():
            content = mod_path.read_text()
            # Try CrtYd fp_rect first
            crtyd = re.findall(
                r'\(fp_rect\s+\(start\s+([-\d.]+)\s+([-\d.]+)\)\s+\(end\s+([-\d.]+)\s+([-\d.]+)\)\s+.*?\([^\)]*CrtYd',
                content,
                re.DOTALL
            )
            if crtyd:
                x1, y1, x2, y2 = map(float, crtyd[0])
                w = round(abs(x2 - x1), 3)
                h = round(abs(y2 - y1), 3)
                if w > 0 and h > 0:
                    return w, h, 0.25
            
            # Try pads bounding box
            pads = re.findall(
                r'\(pad\s+"[^"]*"\s+\w+\s+\w+\s+\(\s*at\s+([-\d.]+)\s+([-\d.]+)\s*\)\s+\(\s*size\s+([-\d.]+)\s+([-\d.]+)\s*\)',
                content
            )
            if pads:
                min_x, max_x, min_y, max_y = 1e9, -1e9, 1e9, -1e9
                for px, py, sx, sy in pads:
                    px, py, sx, sy = float(px), float(py), float(sx), float(sy)
                    min_x = min(min_x, px - sx / 2)
                    max_x = max(max_x, px + sx / 2)
                    min_y = min(min_y, py - sy / 2)
                    max_y = max(max_y, py + sy / 2)
                w = round(max_x - min_x, 3)
                h = round(max_y - min_y, 3)
                if w > 0 and h > 0:
                    return w, h, 0.25

    # Fallback to pattern matching package sizes
    name_check = f"{fp_ref} {filename}".lower()
    if "0201" in name_check:
        return 0.6, 0.3, 0.15
    if "0402" in name_check:
        return 1.0, 0.5, 0.25
    if "0603" in name_check:
        return 1.6, 0.8, 0.25
    if "0805" in name_check:
        return 2.0, 1.25, 0.25
    if "1206" in name_check:
        return 3.2, 1.6, 0.25
    if "1210" in name_check:
        return 3.2, 2.5, 0.25
    if "1812" in name_check:
        return 4.5, 3.2, 0.25
    if "2010" in name_check:
        return 5.0, 2.5, 0.25
    if "2512" in name_check:
        return 6.3, 3.2, 0.25
    if "sot-23" in name_check or "sot23" in name_check:
        return 2.9, 1.3, 0.25
    if "sot-223" in name_check or "sot223" in name_check:
        return 6.5, 3.5, 0.5
    if "soic-8" in name_check or "soic_8" in name_check:
        return 4.9, 3.9, 0.5
    if "tssop" in name_check:
        return 5.0, 4.4, 0.5
    if "qfn" in name_check or "dfn" in name_check:
        m = re.search(r"(\d+)x(\d+)mm", name_check)
        if m:
            return float(m.group(1)), float(m.group(2)), 0.25
        return 7.0, 7.0, 0.25
    if "dip-8" in name_check:
        return 9.5, 6.3, 0.5

    return 5.0, 5.0, 0.25


def get_operating_conditions(part_id: str, kind: str) -> tuple[float | None, float | None, float | None]:
    """Return (min_v, max_v, max_current_ma) for a part."""
    kind = kind.lower()
    pid = part_id.lower()
    
    if kind in ("resistor", "capacitor", "inductor", "passive"):
        if "electrolytic" in pid or "tantalum" in pid:
            return None, 25.0, None
        return None, 50.0, None

    if kind in ("mcu", "microcontroller"):
        if "atmega328" in pid:
            return 1.8, 5.5, 200.0
        return 1.8, 3.6, 150.0

    if kind in ("regulator", "ldo", "charger", "buck"):
        if "5v" in pid:
            return 4.75, 15.0, 1000.0
        if "1v8" in pid or "1v1" in pid:
            return 2.5, 6.0, 500.0
        return 3.5, 15.0, 800.0

    if kind in ("diode", "led"):
        if "led" in pid:
            return 1.8, 3.3, 20.0
        if "zener_3v3" in pid:
            return 3.1, 3.5, 100.0
        if "zener_5v1" in pid:
            return 4.8, 5.4, 100.0
        return None, 100.0, 1000.0

    if kind in ("opamp", "comparator"):
        return 3.0, 32.0, 50.0

    if kind in ("memory", "flash", "eeprom", "sram", "secure_element"):
        return 1.8, 3.6, 50.0

    return 1.8, 5.0, 100.0


def enrich_file(toml_path: Path) -> bool:
    content = toml_path.read_text()
    
    # Extract metadata fields
    id_m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
    kind_m = re.search(r'^kind\s*=\s*"([^"]+)"', content, re.MULTILINE)
    fp_m = re.search(r'^kicad_footprint\s*=\s*"([^"]+)"', content, re.MULTILINE)

    if not id_m or not kind_m:
        return False

    part_id = id_m.group(1)
    kind = kind_m.group(1)
    fp_ref = fp_m.group(1) if fp_m else ""

    # Don't duplicate if section already exists
    has_dims = "[footprint_dimensions]" in content
    has_ops = "[operating_conditions]" in content

    w, h, margin = get_fp_dims(fp_ref, toml_path.name)
    min_v, max_v, max_ma = get_operating_conditions(part_id, kind)

    # Insert dimensions and operating conditions before first [[pins]]
    parts = re.split(r'(?=^\[\[pins\]\])', content, maxsplit=1, flags=re.MULTILINE)
    top_meta = parts[0].rstrip()

    new_meta_lines = []
    if not has_dims:
        new_meta_lines.append("")
        new_meta_lines.append("[footprint_dimensions]")
        new_meta_lines.append(f"width_mm = {w}")
        new_meta_lines.append(f"height_mm = {h}")
        new_meta_lines.append(f"courtyard_margin_mm = {margin}")

    if not has_ops:
        new_meta_lines.append("")
        new_meta_lines.append("[operating_conditions]")
        if min_v is not None:
            new_meta_lines.append(f"min_voltage_v = {min_v}")
        if max_v is not None:
            new_meta_lines.append(f"max_voltage_v = {max_v}")
        if max_ma is not None:
            new_meta_lines.append(f"max_current_ma = {max_ma}")

    new_top = top_meta + "\n" + "\n".join(new_meta_lines) + "\n\n"
    rest = parts[1] if len(parts) > 1 else ""

    # Update unit attribute for opamps/multi-gate ICs if applicable
    if kind in ("opamp", "comparator") and "unit =" not in rest:
        pin_blocks = re.split(r'(?=^\[\[pins\]\])', rest, flags=re.MULTILINE)
        updated_blocks = []
        for block in pin_blocks:
            if not block.strip():
                continue
            name_m = re.search(r'name\s*=\s*"([^"]+)"', block)
            if name_m:
                pname = name_m.group(1)
                unit_val = None
                if "_a" in pname or pname.endswith("_1"):
                    unit_val = "A"
                elif "_b" in pname or pname.endswith("_2"):
                    unit_val = "B"
                elif "_c" in pname or pname.endswith("_3"):
                    unit_val = "C"
                elif "_d" in pname or pname.endswith("_4"):
                    unit_val = "D"
                
                if unit_val:
                    block = block.rstrip() + f'\nunit = "{unit_val}"\n\n'
            updated_blocks.append(block)
        rest = "".join(updated_blocks)

    new_content = new_top + rest
    toml_path.write_text(new_content)
    return True


def main():
    count = 0
    for p in sorted(REGISTRY_ROOT.rglob("*.synth.toml")):
        if enrich_file(p):
            count += 1
    print(f"Enriched {count} registry TOML files with footprint_dimensions, operating_conditions, and unit annotations.")

if __name__ == "__main__":
    main()
