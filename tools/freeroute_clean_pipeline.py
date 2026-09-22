#!/usr/bin/env python3
"""Route a Synth board from a clean netlist and produce a KiCad board."""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


def run(command: list[str]) -> None:
    subprocess.run(command, check=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_board")
    parser.add_argument("output_board")
    parser.add_argument("--jar", required=True)
    parser.add_argument("--java", default="java")
    parser.add_argument("--passes", type=int, default=40)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument(
        "--bottom",
        type=float,
        help="optional final bottom edge in mm after routing (RP2350 default is 73.0)",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix="synth-clean-route-") as work:
        ses = str(Path(work) / "board.ses")
        normalized = str(Path(work) / "normalized.kicad_pcb")
        staged = str(Path(work) / "staged.kicad_pcb")
        refilled = str(Path(work) / "refilled.kicad_pcb")
        run([
            sys.executable, str(root / "fix_kicad_layer_ids.py"),
            args.input_board, normalized,
        ])
        run([
            sys.executable, str(root / "freeroute_autoroute.py"),
            normalized, str(Path(work) / "unused.kicad_pcb"),
            "--jar", args.jar, "--java", args.java,
            "--passes", str(args.passes), "--threads", str(args.threads),
            "--clean-netlist", "--no-import", "--ses-output", ses,
        ])
        run([
            sys.executable, str(root / "import_freerouting_ses_text.py"),
            normalized, ses, staged,
        ])
        run([sys.executable, str(root / "refill_board_zones.py"), staged, refilled])
        run([sys.executable, str(root / "bridge_adjacent_pads.py"), refilled, args.output_board])
        run([
            sys.executable, str(root / "restore_netclasses.py"),
            normalized, args.output_board,
        ])
        if args.bottom is not None:
            compact = str(Path(work) / "compact.kicad_pcb")
            compact_refilled = str(Path(work) / "compact-refilled.kicad_pcb")
            run([
                sys.executable, str(root / "shrink_bottom_outline.py"),
                args.output_board, compact, "--bottom", str(args.bottom),
            ])
            run([sys.executable, str(root / "refill_board_zones.py"), compact, compact_refilled])
            run([
                sys.executable, str(root / "restore_netclasses.py"),
                normalized, compact_refilled,
            ])
            Path(compact_refilled).replace(args.output_board)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
