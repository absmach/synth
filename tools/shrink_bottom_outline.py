#!/usr/bin/env python3
"""Tighten the RP2350 board's unused bottom margin while preserving routing."""

import argparse
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_board")
    parser.add_argument("output_board")
    parser.add_argument("--bottom", type=float, default=73.0)
    args = parser.parse_args()
    text = Path(args.input_board).read_text()
    # The source outline is 91.635 mm tall and its inset zone boundary is
    # 0.5 mm inside it.  Adjust both the outline and the serialized filled
    # zone boundary; KiCad then repours the zones on load/save.
    old_bottom = 91.635
    old_zone = 91.135
    old_filled = {
        "91.1345": args.bottom - 0.0005,
        "91.123294": args.bottom - 0.011706,
        "91.114815": args.bottom - 0.020185,
        "91.062011": args.bottom - 0.072989,
    }
    text = text.replace(f"{old_bottom:.3f}", f"{args.bottom:.3f}")
    text = text.replace(f"{old_zone:.3f}", f"{args.bottom - 0.5:.3f}")
    for old, new in old_filled.items():
        text = text.replace(old, f"{new:.6f}".rstrip("0").rstrip("."))
    Path(args.output_board).write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
