#!/usr/bin/env python3
"""Correct KiCad's reserved copper-layer IDs in an emitted board."""

import argparse
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_board")
    parser.add_argument("output_board")
    args = parser.parse_args()
    text = Path(args.input_board).read_text()
    text = text.replace('(1 "In1.Cu" power)', '(4 "In1.Cu" power)')
    text = text.replace('(2 "In2.Cu" power)', '(6 "In2.Cu" power)')
    text = text.replace('(31 "B.Cu" signal)', '(2 "B.Cu" signal)')
    Path(args.output_board).write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
