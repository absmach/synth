#!/usr/bin/env python3
"""Run FreeRouting on a Synth/KiCad board and import its SES result.

This is intentionally an opt-in post-router. Synth's own partial route is
kept as the input so FreeRouting can improve it, while the caller remains
responsible for native KiCad DRC before accepting the result.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
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


def _bridge_adjacent_same_net_pth_pads(board: pcbnew.BOARD) -> int:
    """Close simple adjacent header-pad gaps after the router finishes.

    Through-hole headers sometimes contain two consecutive pads on the same
    net.  FreeRouting can leave that trivial connection unrouted when the
    surrounding header is already congested.  A short straight bridge is
    safe when the pads are on the same footprint, aligned, non-plane, and at
    the standard 2.54 mm pitch; it does not alter the router's search space.
    """
    plane_net_codes = {zone.GetNetCode() for zone in board.Zones()}
    added = 0
    for footprint in board.GetFootprints():
        pads = [
            pad
            for pad in footprint.Pads()
            if pad.GetAttribute() == 0
            and pad.GetNetCode() > 0
            and pad.GetNetCode() not in plane_net_codes
        ]
        for left_index, first in enumerate(pads):
            for second in pads[left_index + 1 :]:
                if first.GetNetCode() != second.GetNetCode():
                    continue
                start = first.GetPosition()
                end = second.GetPosition()
                dx = abs(end.x - start.x)
                dy = abs(end.y - start.y)
                pitch = max(dx, dy)
                if pitch < pcbnew.FromMM(2.45) or pitch > pcbnew.FromMM(2.65):
                    continue
                if min(dx, dy) > pcbnew.FromMM(0.01):
                    continue
                track = pcbnew.PCB_TRACK(board)
                track.SetStart(start)
                track.SetEnd(end)
                track.SetWidth(pcbnew.FromMM(0.127))
                # Through-hole pads are available on both outer layers.  Put
                # the cleanup bridge on B.Cu so it cannot cross the dense
                # front-side header fanout that FreeRouting already chose.
                track.SetLayer(pcbnew.B_Cu)
                track.SetNetCode(first.GetNetCode())
                board.Add(track)
                added += 1
    return added


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
        source_netclasses = list(
            board.GetDesignSettings().m_NetSettings.GetNetclasses().items()
        )
        export_board = board
        if args.clean_netlist:
            export_board = pcbnew.LoadBoard(args.input_board)
            export_tracks = export_board.Tracks()
            for index in range(export_tracks.size() - 1, -1, -1):
                export_board.Remove(export_tracks[index])
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

        if not pcbnew.ImportSpecctraSES(board, ses):
            raise SystemExit("FreeRouting SES import failed")

        bridged = _bridge_adjacent_same_net_pth_pads(board)
        if bridged:
            print(f"added {bridged} simple adjacent-pad bridges", flush=True)

        # SES import adds copper but does not reliably repour KiCad zones.
        # Without this step every pad connected only through the ground plane
        # is reported as unrouted by native DRC, even though the plane exists.
        if board.Zones():
            pcbnew.ZONE_FILLER(board).Fill(board.Zones())

        # Match the JLC-standard signal class used by the Synth exporter.
        settings = board.GetDesignSettings()
        for name, netclass in source_netclasses:
            settings.m_NetSettings.SetNetclass(name, netclass)
        settings.m_TrackMinWidth = pcbnew.FromMM(0.127)
        settings.m_MinClearance = pcbnew.FromMM(0.127)
        default_class = settings.m_NetSettings.GetDefaultNetclass()
        default_class.SetTrackWidth(pcbnew.FromMM(0.127))
        default_class.SetClearance(pcbnew.FromMM(0.127))

        tracks = board.Tracks()
        for index in range(tracks.size()):
            track = tracks[index]
            if isinstance(track, pcbnew.PCB_VIA):
                continue
            if track.GetWidth() < pcbnew.FromMM(0.127):
                track.SetWidth(pcbnew.FromMM(0.127))

        pcbnew.SaveBoard(args.output_board, board)
        _restore_netclasses(args.input_board, args.output_board)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
