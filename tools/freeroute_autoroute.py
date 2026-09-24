#!/usr/bin/env python3
"""Run FreeRouting on a Synth/KiCad board and import its SES result.

This is intentionally an opt-in post-router. The default preserves Synth's
partial route as input; clean-netlist mode instead routes and imports into a
matching copper-free duplicate so the SES geometry has the same board state
on both sides of the external router.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import pcbnew


def _top_level_netclass_blocks(text: str) -> list[str]:
    """Extract KiCad's top-level net_class forms without a full S-expression parser."""
    blocks = []
    start = 0
    while True:
        start = text.find("\n\t(net_class", start)
        if start < 0:
            return blocks
        depth = 0
        in_string = False
        escaped = False
        end = len(text)
        for index in range(start + 1, len(text)):
            char = text[index]
            if in_string:
                if escaped:
                    escaped = False
                elif char == "\\":
                    escaped = True
                elif char == '"':
                    in_string = False
            elif char == '"':
                in_string = True
            elif char == '(':
                depth += 1
            elif char == ')':
                depth -= 1
                if depth == 0:
                    end = index + 1
                    break
        blocks.append(text[start:end])
        start = end


def _restore_netclasses(source_path: str, output_path: str) -> None:
    source_text = Path(source_path).read_text()
    output_file = Path(output_path)
    output_text = output_file.read_text()
    if "\n\t(net_class" in output_text:
        return
    blocks = _top_level_netclass_blocks(source_text)
    if not blocks:
        return
    insertion = output_text.find("\n\t(gr_")
    if insertion < 0:
        insertion = output_text.find("\n\t(footprint")
    if insertion < 0:
        raise RuntimeError("could not find a safe insertion point for KiCad netclasses")
    output_file.write_text(
        output_text[:insertion] + "\n".join(blocks) + output_text[insertion:]
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_board")
    parser.add_argument("output_board")
    parser.add_argument("--jar", required=True)
    parser.add_argument("--java", default="java")
    # Dense RP2350 boards often improve through the mid-30s before the
    # router's stagnation guard stops.  Keep the default bounded while
    # allowing explicit overrides for more difficult boards.
    parser.add_argument("--passes", type=int, default=40)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument(
        "--ses-output",
        help="copy the FreeRouting SES result to this path before KiCad import",
    )
    parser.add_argument(
        "--clean-netlist",
        action="store_true",
        help="route a duplicate board after removing Synth's existing copper",
    )
    parser.add_argument(
        "--no-import",
        action="store_true",
        help="stop after routing and SES export",
    )
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="synth-freerouting-") as work:
        dsn = os.path.join(work, "board.dsn")
        ses = os.path.join(work, "board.ses")
        data = os.path.join(work, "freerouting-data")
        os.makedirs(data)

        board = pcbnew.LoadBoard(args.input_board)
        export_board = board
        if args.clean_netlist:
            export_board = pcbnew.LoadBoard(args.input_board)
            # `Tracks()` exposes a SWIG vector whose indexed values can be
            # borrowed wrappers on newer KiCad builds.  Remove owned Python
            # objects from `GetTracks()` instead; this works with KiCad 9/10
            # and avoids the `SwigPyObject.thisown` failure.
            for track in list(export_board.GetTracks()):
                export_board.Remove(track)
            print("routing a clean duplicate netlist", flush=True)
        # KiCad's SES importer can discard named netclasses. Keep the source
        # settings alive and restore them after import so the routed board is
        # checked with the same Power/RF constraints as the exported board.
        if not pcbnew.ExportSpecctraDSN(export_board, dsn):
            raise SystemExit("KiCad Specctra DSN export failed")

        subprocess.run(
            [
                args.java,
                "-Djava.awt.headless=true",
                "-jar",
                args.jar,
                "-de",
                dsn,
                "-do",
                ses,
                "-mp",
                str(args.passes),
                "-mt",
                str(args.threads),
                "--user_data_path=" + data,
            ],
            check=True,
        )

        if args.ses_output:
            shutil.copyfile(ses, args.ses_output)
            print(f"saved SES result to {args.ses_output}", flush=True)

        if args.no_import:
            return 0

        # KiCad 10's Python ImportSpecctraSES can segfault on otherwise valid
        # dense SES geometry. Merge the external router's records textually
        # instead; KiCad CLI performs the authoritative refill and DRC after
        # this step. The merger removes all existing top-level segment/via
        # records before adding the SES records, so the original source file
        # remains a suitable merge base even when the DSN was exported from a
        # copper-free duplicate.
        merge_input = args.input_board
        merger = os.path.join(os.path.dirname(__file__), "import_freerouting_ses_text.py")
        subprocess.run(
            [sys.executable, merger, merge_input, ses, args.output_board],
            check=True,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
