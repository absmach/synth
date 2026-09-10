#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""
verify_lcsc.py — Verify LCSC part numbers before adding them to registry TOMLs.

Usage:
    python3 tools/verify_lcsc.py rp2350 C2040

This checks that the given C-number actually exists on lcsc.com using the
browser-accessible product page, then writes the lcsc_id into the matching
registry TOML only if the page resolves.

NOTE: LCSC does not offer an unauthenticated API. This tool checks the human-
readable product URL (https://lcsc.com/product-detail/<id>.html), which is
publicly accessible without auth. It does NOT scrape or parse the page —
it simply confirms the URL returns HTTP 200. The human operator is responsible
for confirming the part description matches before accepting.

How to find a real LCSC C-number:
    1. Go to https://lcsc.com or https://jlcpcb.com/parts
    2. Search by MPN (e.g. "RP2350", "BG95-M3", "nRF52840")
    3. Confirm manufacturer, package, and description
    4. Copy the C-number from the URL or the "LCSC Part #" field
    5. Run this tool with the part_id and C-number
"""

import sys
import glob
import os
import re
import urllib.request
import time

REGISTRY_ROOT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "registry", "parts")


def find_toml(part_id: str) -> str | None:
    for path in glob.glob(REGISTRY_ROOT + "/**/*.synth.toml", recursive=True):
        with open(path) as f:
            content = f.read()
        m = re.search(r'^id\s*=\s*"([^"]+)"', content, re.MULTILINE)
        if m and m.group(1) == part_id:
            return path
    return None


def verify_lcsc_id(lcsc_id: str) -> bool:
    """Return True if the LCSC product page exists (HTTP 200)."""
    url = f"https://lcsc.com/product-detail/{lcsc_id}.html"
    try:
        req = urllib.request.Request(
            url,
            headers={"User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"},
        )
        with urllib.request.urlopen(req, timeout=10) as resp:
            status = resp.status
            return status == 200
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return False
        # Some redirects or other codes — treat as unverifiable
        print(f"  HTTP {e.code} from {url}")
        return False
    except Exception as e:
        print(f"  Network error: {e}")
        return False


def write_lcsc_id(toml_path: str, lcsc_id: str) -> None:
    with open(toml_path) as f:
        content = f.read()

    # Remove existing lcsc_id if present
    lines = [l for l in content.splitlines(True) if not l.strip().startswith("lcsc_id")]

    # Insert after version = line
    new_lines = []
    inserted = False
    for line in lines:
        new_lines.append(line)
        if not inserted and re.match(r'^version\s*=', line.strip()):
            new_lines.append(f'lcsc_id = "{lcsc_id}"\n')
            inserted = True

    if not inserted:
        new_lines.append(f'lcsc_id = "{lcsc_id}"\n')

    with open(toml_path, "w") as f:
        f.writelines(new_lines)


def main():
    if len(sys.argv) != 3:
        print("Usage: python3 tools/verify_lcsc.py <part_id> <C-number>")
        print("  Example: python3 tools/verify_lcsc.py rp2040 C2040")
        print()
        print("How to find C-numbers:")
        print("  1. Search https://lcsc.com for the MPN")
        print("  2. Confirm it's the right part (manufacturer, package)")
        print("  3. Copy the C-number from the product URL or 'LCSC Part #' field")
        sys.exit(1)

    part_id = sys.argv[1].strip()
    lcsc_id = sys.argv[2].strip()

    if not re.match(r'^C\d+$', lcsc_id):
        print(f"Error: '{lcsc_id}' doesn't look like an LCSC C-number (should be C followed by digits)")
        sys.exit(1)

    toml_path = find_toml(part_id)
    if not toml_path:
        print(f"Error: No registry TOML found with id = \"{part_id}\"")
        sys.exit(1)

    print(f"Part ID:   {part_id}")
    print(f"TOML:      {os.path.relpath(toml_path)}")
    print(f"LCSC ID:   {lcsc_id}")
    print(f"Verifying: https://lcsc.com/product-detail/{lcsc_id}.html")

    ok = verify_lcsc_id(lcsc_id)
    if not ok:
        print(f"\n✗ LCSC product page returned 404 or error. Refusing to write.")
        print(f"  Check: https://lcsc.com/product-detail/{lcsc_id}.html")
        sys.exit(1)

    print(f"  HTTP 200 ✓ — page exists")
    print()

    # Ask the operator to confirm before writing
    print("IMPORTANT: Please verify manually that this is the correct part:")
    print(f"  https://lcsc.com/product-detail/{lcsc_id}.html")
    confirm = input("Write lcsc_id to TOML? [y/N] ").strip().lower()
    if confirm != "y":
        print("Cancelled. No changes made.")
        sys.exit(0)

    write_lcsc_id(toml_path, lcsc_id)
    print(f"✓ Written lcsc_id = \"{lcsc_id}\" to {os.path.relpath(toml_path)}")


if __name__ == "__main__":
    main()
