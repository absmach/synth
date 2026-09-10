#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
import_lcsc.py — Backfill verified LCSC C-numbers into registry TOMLs.

IMPORTANT: Every C-number in LCSC_VERIFIED below MUST be manually confirmed
on https://lcsc.com before being added here. Use tools/verify_lcsc.py to
check individual parts interactively.

No fallback / generated IDs. A missing entry means the operator has not yet
verified that part. That is correct and intentional — missing is better than wrong.
"""

import glob
import os
import re

# -----------------------------------------------------------------------
# Manually verified LCSC catalog numbers.
# To add a new part:
#   1. Search https://lcsc.com for the MPN
#   2. Confirm manufacturer + package + description match the registry TOML
#   3. Run: python3 tools/verify_lcsc.py <part_id> <C-number>
#   4. If the tool confirms HTTP 200, add the entry here
# -----------------------------------------------------------------------
LCSC_VERIFIED: dict[str, str] = {
    # NOTE: This mapping is intentionally sparse.
    # Parts will be added here one at a time after manual verification.
    # Do NOT add C-numbers from memory or AI suggestions without
    # confirming them on https://lcsc.com first.
}


def main() -> None:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    parts_dir = os.path.join(root, "registry", "parts")
    updated = 0
    skipped = 0

    for filepath in sorted(glob.glob(os.path.join(parts_dir, "**", "*.synth.toml"), recursive=True)):
        with open(filepath) as f:
            content = f.read()

        m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if not m:
            continue
        part_id = m.group(1)

        real_lcsc = LCSC_VERIFIED.get(part_id)
        if not real_lcsc:
            skipped += 1
            continue

        # Remove any stale lcsc_id line first
        lines = [l for l in content.splitlines(True) if not l.strip().startswith("lcsc_id =")]

        new_lines: list[str] = []
        inserted = False
        for line in lines:
            new_lines.append(line)
            if not inserted and re.match(r'^version\s*=', line.strip()):
                new_lines.append(f'lcsc_id = "{real_lcsc}"\n')
                inserted = True
        if not inserted:
            new_lines.append(f'lcsc_id = "{real_lcsc}"\n')

        with open(filepath, "w") as f:
            f.writelines(new_lines)
        updated += 1
        print(f"  {part_id}: {real_lcsc}")

    print(f"\n{updated} parts updated with verified LCSC IDs; {skipped} parts skipped (not yet verified).")


if __name__ == "__main__":
    main()
