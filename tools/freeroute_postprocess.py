#!/usr/bin/env python3
"""Post-process a FreeRouting-imported board in a fresh KiCad process."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import pcbnew

from freeroute_autoroute import all_tracks, stitch_floating_pour, tie_distant_ground_pads


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("board")
    parser.add_argument("--allow-via-in-pad", action="store_true")
    parser.add_argument("--report")
    args = parser.parse_args()

    stitched, unresolved = stitch_floating_pour(args.board, args.allow_via_in_pad)
    tied, tie_unresolved = tie_distant_ground_pads(args.board)
    board = pcbnew.LoadBoard(args.board)
    tracks = all_tracks(board)
    segments = sum(1 for track in tracks if track.GetClass().endswith("TRACK"))
    vias = sum(1 for track in tracks if track.GetClass().endswith("VIA"))
    report = {
        "router": "freerouting",
        "imported_segments": segments,
        "imported_vias": vias,
        "stitch_vias": len(stitched),
        "stitch_unresolved": unresolved,
        "ground_vias": len(tied),
        "ground_unresolved": tie_unresolved,
        "notes": [],
    }
    if args.report:
        Path(args.report).write_text(json.dumps(report, indent=2, sort_keys=True))
    print(json.dumps(report, sort_keys=True), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
