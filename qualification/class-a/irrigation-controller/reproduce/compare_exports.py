#!/usr/bin/env python3
"""compare_exports.py <dir>: compare <dir>/out1, out2 and out3, three exports of one board.

Prints the first ten characters of each file's SHA-256 per export. The routed
board is also hashed with the random ids the router gives its tracks, vias and
zone fills masked, because those differ between runs while the geometry does
not. Exits 1 when a file other than the routed board differs, or when the masked
routed boards differ."""
import hashlib
import re
import sys
from pathlib import Path

FILES = [
    "irrigation_controller.kicad_sch",
    "irrigation_controller.kicad_pro",
    "irrigation_controller.kicad_sym",
    "bom.csv",
    "pnp.csv",
    "irrigation_controller.synth.kicad_pcb",
]
PCB = "irrigation_controller.kicad_pcb"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:10]


def mask(text: str) -> str:
    """The routed board with the uuid of every segment, via and zone replaced."""
    out, i = [], 0
    pattern = re.compile(r"\((?:segment|via|zone)\b")
    while True:
        m = pattern.search(text, i)
        if not m:
            out.append(text[i:])
            return "".join(out)
        start, depth, k = m.start(), 0, m.start()
        out.append(text[i:start])
        while True:
            c = text[k]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        out.append(re.sub(r'\(uuid "[^"]*"\)', '(uuid "-")', text[start : k + 1]))
        i = k + 1


def main() -> int:
    base = Path(sys.argv[1]).expanduser()
    outs = [base / f"out{n}" for n in (1, 2, 3)]
    failed = False
    for name in FILES:
        hashes = [digest((o / name).read_bytes()) for o in outs]
        same = len(set(hashes)) == 1
        failed |= not same
        print(f"{name:45} {' '.join(hashes)}  {'identical' if same else 'DIFFERENT'}")
    masked = [digest(mask((o / PCB).read_text()).encode()) for o in outs]
    same = len(set(masked)) == 1
    failed |= not same
    print(f"{PCB + ' (ids masked)':45} {' '.join(masked)}  {'identical' if same else 'DIFFERENT'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
