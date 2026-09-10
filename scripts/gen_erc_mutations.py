#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
Generates ERC-broken design fixtures into fixtures/erc-generated/
by applying single and double programmatic mutations to clean .synth designs.
Each mutation triggers specific ERC diagnostic codes.
"""

import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

def mutate_float_required_pin(src: str) -> str:
    lines = src.splitlines()
    new_lines = []
    removed = False
    for line in lines:
        if not removed and any(kw in line for kw in ["vdd_io", "vdd_core", "vcc", "vbat", "vdd", "vin"]) and "connect" in line:
            new_lines.append(f"  // MUTATION: {line}")
            removed = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if removed else None

def mutate_single_endpoint_net(src: str) -> str:
    lines = src.splitlines()
    new_lines = []
    removed = False
    for line in lines:
        if not removed and "connect" in line and "->" in line:
            parts = line.split("->")
            new_lines.append(f"  // MUTATION\n  {parts[0].strip()}")
            removed = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if removed else None

def mutate_power_short(src: str) -> str:
    lines = src.splitlines()
    idx = len(lines) - 2
    lines.insert(idx, "  // MUTATION E-SYNTH-POWER-002\n  connect U3.vout -> U4.vout")
    return "\n".join(lines)

def mutate_missing_decoupling(src: str) -> str:
    lines = src.splitlines()
    new_lines = []
    removed = 0
    for line in lines:
        if "connect" in line and (".p1" in line or ".p2" in line) and any(c in line for c in ["C1", "C2", "C3", "C4"]):
            new_lines.append(f"  // MUTATION: {line}")
            removed += 1
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if removed > 0 else None

def mutate_strip_diff_impedance(src: str) -> str:
    if "diff_pair" not in src:
        return None
    lines = src.splitlines()
    new_lines = []
    modified = False
    for line in lines:
        if "impedance" in line:
            new_lines.append(f"  // MUTATION: {line}")
            modified = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if modified else None

def mutate_diff_self_ref(src: str) -> str:
    if "diff_pair" not in src:
        return None
    return re.sub(r'diff_pair\s+([A-Za-z0-9_]+)\s+([A-Za-z0-9_]+)', r'diff_pair \1 \1', src)

def mutate_drop_i2c_pullup(src: str) -> str:
    if "sda" not in src.lower() and "scl" not in src.lower():
        return None
    lines = src.splitlines()
    new_lines = []
    modified = False
    for line in lines:
        if ("sda" in line.lower() or "scl" in line.lower()) and any(r in line for r in ["R1", "R2", "R3", "R4"]):
            new_lines.append(f"  // MUTATION: {line}")
            modified = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if modified else None

def mutate_remove_keepout_radius(src: str) -> str:
    if "keepout" not in src:
        return None
    lines = src.splitlines()
    new_lines = []
    modified = False
    for line in lines:
        if "radius" in line:
            new_lines.append(f"  // MUTATION: {line}")
            modified = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if modified else None

def mutate_zero_keepout_radius(src: str) -> str:
    if "keepout" not in src:
        return None
    return re.sub(r'radius\s+[0-9\.]+[a-z]+', 'radius 0mm', src)

def mutate_remove_cc_pulldown(src: str) -> str:
    if "cc1" not in src.lower() and "cc2" not in src.lower():
        return None
    lines = src.splitlines()
    new_lines = []
    modified = False
    for line in lines:
        if "cc1" in line.lower() or "cc2" in line.lower():
            new_lines.append(f"  // MUTATION: {line}")
            modified = True
        else:
            new_lines.append(line)
    return "\n".join(new_lines) if modified else None

ALL_MUTATIONS = [
    ("float_required", mutate_float_required_pin),
    ("single_endpoint", mutate_single_endpoint_net),
    ("power_short", mutate_power_short),
    ("missing_decoupling", mutate_missing_decoupling),
    ("strip_diff_impedance", mutate_strip_diff_impedance),
    ("diff_self_ref", mutate_diff_self_ref),
    ("drop_i2c_pullup", mutate_drop_i2c_pullup),
    ("remove_keepout_radius", mutate_remove_keepout_radius),
    ("zero_keepout_radius", mutate_zero_keepout_radius),
    ("remove_cc_pulldown", mutate_remove_cc_pulldown),
]

def main():
    out_dir = REPO_ROOT / "fixtures" / "erc-generated"
    out_dir.mkdir(parents=True, exist_ok=True)

    input_dirs = [
        REPO_ROOT / "fixtures" / "designs",
        REPO_ROOT / "fixtures" / "designs-generated",
        REPO_ROOT / "fixtures" / "kicad-reference",
        REPO_ROOT / "fixtures" / "erc",
    ]

    clean_files = []
    for d in input_dirs:
        if d.exists():
            for p in d.glob("*.synth"):
                if "pass__" in p.name or d.name.startswith("designs") or d.name.startswith("kicad"):
                    clean_files.append(p)

    print(f"Applying ERC single & double mutations to {len(clean_files)} clean design files...")

    generated_mutations = 0

    for file_path in clean_files:
        try:
            src = file_path.read_text(encoding="utf-8")
        except Exception:
            continue

        stem = file_path.stem

        # 1. Single mutations
        single_results = []
        for mut_name, mut_fn in ALL_MUTATIONS:
            mutated_src = mut_fn(src)
            if mutated_src and mutated_src != src:
                out_name = f"{stem}_mut_{mut_name}.synth"
                out_file = out_dir / out_name
                out_file.write_text(mutated_src, encoding="utf-8")
                generated_mutations += 1
                single_results.append((mut_name, mutated_src))

        # 2. Double mutations (combinations of 2 distinct single mutations)
        for i in range(len(single_results)):
            for j in range(i + 1, len(single_results)):
                name_i, src_i = single_results[i]
                name_j, _ = single_results[j]
                mut_fn_j = dict(ALL_MUTATIONS)[name_j]
                double_src = mut_fn_j(src_i)
                if double_src and double_src != src_i:
                    out_name = f"{stem}_double_{name_i}_{name_j}.synth"
                    out_file = out_dir / out_name
                    out_file.write_text(double_src, encoding="utf-8")
                    generated_mutations += 1

    print(f"Successfully generated {generated_mutations} mutated ERC fixture files into {out_dir}.")

if __name__ == "__main__":
    main()
