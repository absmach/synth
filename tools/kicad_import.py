#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
kicad_import.py — Extract verified pin data from your LOCAL KiCad symbol libraries
and regenerate registry TOMLs with correct pin names, numbers, and electrical types.

This is the ONLY trustworthy source for pin data in this project.
All pin numbers and electrical types come directly from /usr/share/kicad/symbols/.

Usage:
    # Audit all parts — print mismatches without writing anything
    python3 tools/kicad_import.py --audit

    # Regenerate a specific part's pins from KiCad
    python3 tools/kicad_import.py --update rp2350

    # Regenerate ALL parts that have a kicad_symbol reference
    python3 tools/kicad_import.py --update-all

KiCad electrical type -> synth electrical type mapping:
    power_in    -> power_input
    power_out   -> power_output
    input       -> input
    output      -> output
    bidirectional -> bidirectional
    passive     -> passive
    no_connect  -> (skipped — NC pins are omitted)
    open_collector / open_emitter -> output
"""

import argparse
import re
import glob
import os
import sys
from pathlib import Path

KICAD_SYM_ROOT = Path("/usr/share/kicad/symbols")
REGISTRY_ROOT = Path(__file__).parent.parent / "registry" / "parts"

# Map KiCad electrical type strings to synth electrical type strings
KICAD_TO_SYNTH_ETYPE: dict[str, str] = {
    "power_in": "power_input",
    "power_out": "power_output",
    "pwrflag": "power_input",
    "input": "input",
    "output": "output",
    "bidirectional": "bidirectional",
    "passive": "passive",
    "tri_state": "bidirectional",
    "open_collector": "output",
    "open_emitter": "output",
    "unspecified": "passive",
}

# Pin names that indicate no-connect (skipped)
NC_NAMES = {"NC", "~", "", "~{NC}"}


def _extract_pins_from_block(block: str) -> list[tuple[str, str, str]]:
    """Extract (kicad_type, name, number) from an s-expression symbol block."""
    pin_pat = re.compile(
        r'\(pin\s+(\w+)\s+\w+\s*'         # (pin <type> <style>
        r'\(\s*at\s[^\)]+\)\s*'            # (at x y angle)
        r'\(\s*length\s[^\)]+\)\s*'        # (length n)
        r'\(\s*name\s+"([^"]*)".*?'        # (name "NAME" ...)
        r'\(\s*number\s+"([^"]*)"',        # (number "NUM"
        re.DOTALL,
    )
    return pin_pat.findall(block)


def parse_kicad_sym(sym_file: Path, sym_name: str, _visited: set[str] | None = None) -> list[dict] | None:
    """
    Parse a .kicad_sym file and return all pins for the named symbol.
    Handles KiCad symbol inheritance via `(extends "ParentSymbol")`.
    Returns list of dicts: {name, number, kicad_type, synth_type}
    Returns None if symbol not found.
    """
    if _visited is None:
        _visited = set()
    if sym_name in _visited:
        return None  # Cycle guard
    _visited.add(sym_name)

    content = sym_file.read_text()

    # Find the symbol block — search only at top-level (symbol "name" ...) entries
    # to avoid matching sub-symbol blocks named "name_0_1" etc.
    pattern = re.compile(r'^\s*\(symbol\s+"' + re.escape(sym_name) + r'"', re.MULTILINE)
    m = pattern.search(content)
    if not m:
        return None

    start = m.start()

    # Extract the symbol block by counting parenthesis depth.
    # This correctly bounds us to exactly this symbol without spilling into the next.
    depth = 0
    i = start
    block_end = len(content)
    while i < len(content):
        c = content[i]
        if c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0:
                block_end = i + 1
                break
        i += 1
    block = content[start:block_end]

    # Check for `(extends "ParentSymbol")` — only look in header (first 1000 chars),
    # not in nested sub-symbol blocks which may themselves extend the parent.
    header = block[:1000]
    extends_m = re.search(r'\(extends\s+"([^"]+)"\)', header)
    if extends_m:
        parent_name = extends_m.group(1)
        return parse_kicad_sym(sym_file, parent_name, _visited)

    raw_pins = _extract_pins_from_block(block)


    seen: set[str] = set()  # deduplicate by pin number
    pins = []
    for kicad_type, name, number in raw_pins:
        if number in seen:
            continue
        seen.add(number)
        if kicad_type == "no_connect":
            continue
        # Keep empty-named pins (e.g. Device:C pins 1 and 2) — use number as fallback name
        if name in NC_NAMES:
            name = f"pin{number}"
        synth_type = KICAD_TO_SYNTH_ETYPE.get(kicad_type, "passive")
        pins.append(
            {
                "name": name,
                "number": number,
                "kicad_type": kicad_type,
                "synth_type": synth_type,
            }
        )
    return pins




def load_toml_metadata(toml_path: Path) -> dict:
    """
    Extract the non-pin metadata fields from a registry TOML.
    Returns a dict of raw field lines grouped by section.
    """
    content = toml_path.read_text()
    meta: dict[str, str] = {}

    # Top-level scalar fields (everything before first [[pins]])
    top_section = re.split(r'^\[\[pins\]\]', content, maxsplit=1, flags=re.MULTILINE)[0]
    for line in top_section.splitlines():
        m = re.match(r'^(\w+)\s*=\s*(.+)', line.strip())
        if m:
            meta[m.group(1)] = m.group(2)

    # required_decoupling blocks
    meta["_required_decoupling_raw"] = re.findall(
        r'\[\[required_decoupling\]\].*?(?=\[\[|\Z)', content, re.DOTALL
    )
    return meta


def synth_type_to_required(synth_type: str) -> bool:
    """Power pins are required by default; everything else is not."""
    return synth_type in ("power_input", "power_output")


def sanitize_pin_name(name: str, number: str = "") -> str:
    """
    Convert KiCad pin name to a clean synth pin name.
    """
    if name == "+":
        return "pos"
    if name == "-":
        return "neg"
    # Strip KiCad overbar notation ~{...}
    name = re.sub(r'~\{([^}]+)\}', r'\1', name)
    name = name.lower()

    if name in ("d+", "d_p", "usb_d+", "usb_dp"):
        name = "dp"
    elif name in ("d-", "d_n", "usb_d-", "usb_dn"):
        name = "dn"
    else:
        name = name.replace('+', '_plus').replace('-', '_minus')

    # Replace non-alphanumeric chars with underscore
    name = re.sub(r'[^a-z0-9]+', '_', name)
    name = re.sub(r'_+', '_', name)
    name = name.strip('_')
    if not name and number:
        return f"pin{number}"
    return name


def pins_to_toml_blocks(pins: list[dict], existing_capabilities: dict[str, list[str]] = None) -> str:
    """
    Render a list of pin dicts to TOML [[pins]] blocks.
    `existing_capabilities` maps sanitized_name -> capabilities list from old TOML.
    """
    existing_capabilities = existing_capabilities or {}
    lines = []
    seen_names: set[str] = set()
    for pin in pins:
        clean_name = sanitize_pin_name(pin["name"], pin["number"])
        if clean_name in seen_names:
            clean_name = f"{clean_name}_{pin['number'].lower()}"
        seen_names.add(clean_name)

        lines.append("[[pins]]")
        lines.append(f'name = "{clean_name}"')
        lines.append(f'number = "{pin["number"]}"')
        lines.append(f'electrical_type = "{pin["synth_type"]}"')

        # Preserve capabilities from old TOML if pin name or number matches
        caps = existing_capabilities.get(clean_name) or existing_capabilities.get(pin["name"]) or existing_capabilities.get(pin["number"])
        if caps:
            caps_str = ", ".join(f'"{c}"' for c in caps)
            lines.append(f'capabilities = [{caps_str}]')

        if synth_type_to_required(pin["synth_type"]):
            lines.append("required = true")
        lines.append("")
    return "\n".join(lines)


def extract_existing_capabilities(toml_path: Path) -> dict[str, list[str]]:
    """Pull capability lists from existing TOML keyed by pin name and number."""
    content = toml_path.read_text()
    result: dict[str, list[str]] = {}
    blocks = re.findall(
        r'\[\[pins\]\](.*?)(?=\[\[pins\]\]|\[\[required|\Z)', content, re.DOTALL
    )
    for block in blocks:
        name_m = re.search(r'name\s*=\s*"([^"]+)"', block)
        num_m = re.search(r'number\s*=\s*"([^"]+)"', block)
        cap_m = re.search(r'capabilities\s*=\s*\[([^\]]+)\]', block)
        if cap_m:
            caps = [c.strip().strip('"') for c in cap_m.group(1).split(",")]
            if name_m:
                result[name_m.group(1)] = caps
            if num_m:
                result[num_m.group(1)] = caps
    return result


def rebuild_toml(toml_path: Path, kicad_pins: list[dict], dry_run: bool = False) -> str:
    """
    Rebuild a registry TOML: keep all metadata fields, replace [[pins]] blocks
    with data from KiCad. Returns the new content string.
    """
    content = toml_path.read_text()

    # Split into: [metadata_section, ...pin_blocks..., trailing]
    # Everything before the first [[pins]] is metadata
    parts = re.split(r'(?=^\[\[pins\]\])', content, maxsplit=1, flags=re.MULTILINE)
    metadata_section = parts[0].rstrip("\n") + "\n\n"

    # Extract existing capabilities before we overwrite
    existing_caps = extract_existing_capabilities(toml_path)

    # Extract required_decoupling from original
    decoupling_blocks = re.findall(
        r'(\[\[required_decoupling\]\].*?)(?=\[\[required_decoupling\]\]|\Z)',
        content,
        re.DOTALL,
    )

    pin_section = pins_to_toml_blocks(kicad_pins, existing_caps)
    decoupling_section = "\n".join(b.rstrip() for b in decoupling_blocks)
    if decoupling_section:
        decoupling_section = "\n" + decoupling_section + "\n"

    new_content = metadata_section + pin_section + decoupling_section
    return new_content


def find_toml_by_id(part_id: str) -> Path | None:
    for p in REGISTRY_ROOT.rglob("*.synth.toml"):
        content = p.read_text()
        m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if m and m.group(1) == part_id:
            return p
    return None


def get_kicad_sym_ref(toml_path: Path) -> tuple[str, str] | None:
    """Return (lib_name, sym_name) from kicad_symbol field, or None."""
    content = toml_path.read_text()
    m = re.search(r'^kicad_symbol\s*=\s*"([^"]+)"', content, re.MULTILINE)
    if not m:
        return None
    ref = m.group(1)
    if ":" not in ref:
        return None
    lib, sym = ref.split(":", 1)
    return lib, sym


def audit_all() -> None:
    """Print a report of data quality for all registry parts."""
    ok, mismatch, no_sym = [], [], []
    for toml_path in sorted(REGISTRY_ROOT.rglob("*.synth.toml")):
        content = toml_path.read_text()
        id_m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
        part_id = id_m.group(1) if id_m else toml_path.stem

        sym_ref = get_kicad_sym_ref(toml_path)
        if not sym_ref:
            no_sym.append(part_id)
            continue

        lib, sym = sym_ref
        sym_file = KICAD_SYM_ROOT / f"{lib}.kicad_sym"
        if not sym_file.exists():
            no_sym.append(f"{part_id} (lib file missing: {lib})")
            continue

        kicad_pins = parse_kicad_sym(sym_file, sym)
        if kicad_pins is None:
            no_sym.append(f"{part_id} (symbol '{sym}' not found in {lib})")
            continue

        toml_pins = re.findall(
            r'\[\[pins\]\].*?number\s*=\s*"([^"]+)"', content, re.DOTALL
        )
        kicad_nums = {p["number"] for p in kicad_pins}
        bad = [n for n in toml_pins if n not in kicad_nums]

        if bad:
            mismatch.append((part_id, bad, len(toml_pins), len(kicad_pins)))
        else:
            ok.append((part_id, len(kicad_pins)))

    print(f"{'='*60}")
    print(f" KICAD DATA AUDIT — {len(ok)+len(mismatch)+len(no_sym)} parts total")
    print(f"{'='*60}")
    print(f"\n✓ VERIFIED ({len(ok)} parts — pin numbers match KiCad exactly):")
    for part_id, n in ok:
        print(f"    {part_id:<35} ({n} pins in KiCad)")

    print(f"\n✗ MISMATCH ({len(mismatch)} parts — TOML pin numbers not in KiCad):")
    for part_id, bad_nums, toml_total, kicad_total in mismatch:
        print(f"    {part_id:<35} bad pin#s: {bad_nums}  "
              f"(TOML:{toml_total} KiCad:{kicad_total})")
        print(f"      → Run: python3 tools/kicad_import.py --update {part_id}")

    print(f"\n? NO SYMBOL ({len(no_sym)} parts — need manual data entry or EasyEDA):")
    for part_id in no_sym:
        print(f"    {part_id}")

    print()


def update_part(part_id: str, dry_run: bool = False) -> None:
    toml_path = find_toml_by_id(part_id)
    if not toml_path:
        print(f"Error: no TOML found with id = \"{part_id}\"")
        sys.exit(1)

    sym_ref = get_kicad_sym_ref(toml_path)
    if not sym_ref:
        print(f"Error: {part_id} has no kicad_symbol field — cannot auto-import")
        sys.exit(1)

    lib, sym = sym_ref
    sym_file = KICAD_SYM_ROOT / f"{lib}.kicad_sym"
    if not sym_file.exists():
        print(f"Error: KiCad library file not found: {sym_file}")
        sys.exit(1)

    kicad_pins = parse_kicad_sym(sym_file, sym)
    if kicad_pins is None:
        print(f"Error: symbol '{sym}' not found in {sym_file.name}")
        print(f"  Available symbols containing the keyword:")
        content = sym_file.read_text()
        keyword = sym.split("-")[0]  # try partial match
        found = re.findall(r'\(symbol "([^"]*' + re.escape(keyword) + r'[^"]*)"', content, re.IGNORECASE)
        for f in found[:10]:
            print(f"    {f}")
        sys.exit(1)

    print(f"Part:   {part_id}")
    print(f"Symbol: {lib}:{sym}")
    print(f"Pins from KiCad: {len(kicad_pins)}")
    for p in kicad_pins[:10]:
        print(f"  {p['number']:<6} {p['name']:<30} ({p['kicad_type']} → {p['synth_type']})")
    if len(kicad_pins) > 10:
        print(f"  ... and {len(kicad_pins)-10} more")

    new_content = rebuild_toml(toml_path, kicad_pins, dry_run=dry_run)

    if dry_run:
        print(f"\n--- DRY RUN: would write to {toml_path.relative_to(REGISTRY_ROOT.parent.parent)} ---")
        print(new_content[:800])
        return

    toml_path.write_text(new_content)
    print(f"\n✓ Written {len(kicad_pins)} KiCad-verified pins to {toml_path.relative_to(REGISTRY_ROOT.parent.parent)}")


def update_all(dry_run: bool = False) -> None:
    updated, skipped, errors = [], [], []
    for toml_path in sorted(REGISTRY_ROOT.rglob("*.synth.toml")):
        content = toml_path.read_text()
        id_m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
        part_id = id_m.group(1) if id_m else toml_path.stem

        sym_ref = get_kicad_sym_ref(toml_path)
        if not sym_ref:
            skipped.append(part_id)
            continue

        lib, sym = sym_ref
        sym_file = KICAD_SYM_ROOT / f"{lib}.kicad_sym"
        if not sym_file.exists():
            errors.append(f"{part_id}: lib file missing: {lib}")
            continue

        kicad_pins = parse_kicad_sym(sym_file, sym)
        if kicad_pins is None:
            errors.append(f"{part_id}: symbol '{sym}' not in {lib}")
            continue

        new_content = rebuild_toml(toml_path, kicad_pins)
        if not dry_run:
            toml_path.write_text(new_content)
        updated.append(f"{part_id} ({len(kicad_pins)} pins)")

    print(f"Updated {len(updated)} parts with KiCad-verified pin data:")
    for u in updated:
        print(f"  ✓ {u}")
    if errors:
        print(f"\nErrors ({len(errors)}) — need manual kicad_symbol fix:")
        for e in errors:
            print(f"  ✗ {e}")
    if skipped:
        print(f"\nSkipped ({len(skipped)}) — no kicad_symbol field:")
        for s in skipped:
            print(f"  ? {s}")


def main():
    parser = argparse.ArgumentParser(
        description="Import verified pin data from local KiCad symbol libraries into registry TOMLs."
    )
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--audit", action="store_true", help="Audit all parts without writing")
    group.add_argument("--update", metavar="PART_ID", help="Update pins for one part")
    group.add_argument("--update-all", action="store_true", help="Update pins for all parts with kicad_symbol")
    parser.add_argument("--dry-run", action="store_true", help="Print output without writing files")
    args = parser.parse_args()

    if args.audit:
        audit_all()
    elif args.update:
        update_part(args.update, dry_run=args.dry_run)
    elif args.update_all:
        update_all(dry_run=args.dry_run)


if __name__ == "__main__":
    main()
