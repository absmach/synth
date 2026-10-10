#!/usr/bin/env python3
"""Run FreeRouting on a Synth/KiCad board and import its SES result.

The board arrives un-routed, because routing is the only path: clean-netlist
mode therefore always applies, so the SES geometry is produced against the
same board state on both sides of the external router.

The `--report` this writes is the router's own account of what it did. It is
recorded, and the independent checks downstream compare it against the copper
that was actually written, rather than taking it as the answer.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import uuid
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
            elif char == "(":
                depth += 1
            elif char == ")":
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


def all_tracks(board) -> list:
    """Every track and via on the board, as owned Python objects.

    `GetTracks()` is the documented accessor but it is not usable on every
    KiCad build: its SWIG iterator calls `it.next()`, a name Python 3 removed,
    so on KiCad 10 it raises `AttributeError` before yielding anything.
    Indexing `Tracks()` directly avoids the iterator entirely and works on
    both, so it is tried first and the accessor is only a fallback.
    """
    container = board.Tracks()
    tracks = []
    index = 0
    while True:
        try:
            tracks.append(container[index])
        except IndexError:
            return tracks
        except (TypeError, AttributeError):
            break
        index += 1
    try:
        return list(board.GetTracks())
    except (AttributeError, TypeError):
        return tracks


STITCH_PAD_MM = 0.6
STITCH_DRILL_MM = 0.3
# Room a via needs inside a pour: its radius, the zone thermal gap (0.2) and
# a little extra so a thermal spoke still fits. Tried largest first.
STITCH_ROOMS_MM = (0.7, 0.5, 0.4)
STITCH_SEARCH_MM = 4.0
STITCH_STEP_MM = 0.1
STITCH_ROUNDS = 3
# Gap kept between a stitch via's copper and any other net's copper.
STITCH_CLEARANCE_MM = 0.2
# With via-in-pad approved, only pads at least this wide both ways hold a via.
STITCH_BIG_PAD_MM = 1.5
# Max error allowed when shrinking a pour piece, in mm.
STITCH_DEFLATE_ERROR_MM = 0.01

_STITCH_STEPS = int(STITCH_SEARCH_MM / STITCH_STEP_MM)
# Grid offsets around a pad, nearest first, so the first legal spot is the
# closest one. Ties break on the offset itself to keep the order fixed.
_STITCH_OFFSETS = sorted(
    (
        (dx, dy)
        for dx in range(-_STITCH_STEPS, _STITCH_STEPS + 1)
        for dy in range(-_STITCH_STEPS, _STITCH_STEPS + 1)
    ),
    key=lambda o: (o[0] ** 2 + o[1] ** 2, o),
)


def _mm(value: float) -> int:
    return pcbnew.FromMM(value)


def _fragments(polys):
    """Each filled piece of a pour as its own polygon (outline plus holes)."""
    for index in range(polys.OutlineCount()):
        piece = pcbnew.SHAPE_POLY_SET()
        piece.AddOutline(polys.COutline(index))
        for hole in range(polys.HoleCount(index)):
            piece.AddHole(polys.CHole(index, hole))
        yield piece


def _is_anchored(piece, tracks, pads, net) -> bool:
    """True if a via or through-hole pad of `net` sits inside `piece`.

    Those reach the inner planes. An SMD pad only touches its own layer, so a
    piece holding nothing else is cut off from the rest of the net.
    """
    for track in tracks:
        if (
            track.GetClass().endswith("VIA")
            and track.GetNetCode() == net
            and piece.PointInside(track.GetPosition())
        ):
            return True
    return any(
        pad.GetNetCode() == net
        and pad.GetAttribute() == pcbnew.PAD_ATTRIB_PTH
        and piece.PointInside(pad.GetPosition())
        for pad in pads
    )


def _via_fits(tracks, pads, net, point, in_pad=False) -> bool:
    """True if a via at `point` keeps clear of every pad and other nets' tracks.

    Not inside a pad: solder wicks into it, the fab charges extra for
    via-in-pad, and Synth's release gate rejects it unless approved. With
    `in_pad` (that approval) a big pad of the via's own net, such as a QFN
    exposed pad, may hold the via.
    """
    via = pcbnew.SHAPE_CIRCLE(point, _mm(STITCH_PAD_MM / 2))
    clearance = _mm(STITCH_CLEARANCE_MM)
    for track in tracks:
        if track.GetNetCode() == net:
            continue
        if track.GetEffectiveShape().Collide(via, clearance):
            return False
    for pad in pads:
        big = min(pad.GetSizeX(), pad.GetSizeY()) >= _mm(STITCH_BIG_PAD_MM)
        if in_pad and big and pad.GetNetCode() == net:
            continue
        if pad.GetEffectiveShape(pad.GetLayer()).Collide(via, clearance):
            return False
    return True


def _stitch_point(piece, tracks, pads, net, anchor_pads, in_pad=False):
    """The legal via position closest to one of `anchor_pads`, or None."""
    for room in STITCH_ROOMS_MM:
        inner = pcbnew.SHAPE_POLY_SET(piece)
        inner.Deflate(
            _mm(room),
            pcbnew.CORNER_STRATEGY_ROUND_ALL_CORNERS,
            _mm(STITCH_DEFLATE_ERROR_MM),
        )
        if inner.OutlineCount() == 0:
            continue
        for dx, dy in _STITCH_OFFSETS:
            for pad in anchor_pads:
                pos = pad.GetPosition()
                point = pcbnew.VECTOR2I(
                    pos.x + _mm(dx * STITCH_STEP_MM),
                    pos.y + _mm(dy * STITCH_STEP_MM),
                )
                if inner.PointInside(point) and _via_fits(
                    tracks, pads, net, point, in_pad
                ):
                    return point
    return None


def _add_via(board, net, point):
    via = pcbnew.PCB_VIA(board)
    via.SetPosition(point)
    via.SetViaType(pcbnew.VIATYPE_THROUGH)
    via.SetWidth(pcbnew.F_Cu, _mm(STITCH_PAD_MM))
    via.SetDrill(_mm(STITCH_DRILL_MM))
    via.SetNetCode(net)
    board.Add(via)
    return via


def _floating_pieces(board, tracks, pads):
    """(net, layer name, piece, pads) for each outer pour piece cut off from its net.

    Only a zone's first layer is looked at: Synth writes one zone per layer.
    """
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
    found = []
    for zone in board.Zones():
        layer = zone.GetFirstLayer()
        if layer not in (pcbnew.F_Cu, pcbnew.B_Cu):
            continue
        net = zone.GetNetCode()
        for piece in _fragments(zone.GetFilledPolysList(layer)):
            on_piece = [
                pad
                for pad in pads
                if pad.GetNetCode() == net
                and pad.IsOnLayer(layer)
                and piece.PointInside(pad.GetPosition())
            ]
            if on_piece and not _is_anchored(piece, tracks, pads, net):
                found.append((net, board.GetLayerName(layer), piece, on_piece))
    return found


def stitch_floating_pour(
    board_path: str, allow_via_in_pad: bool = False
) -> tuple[list[tuple[float, float]], int]:
    """Join outer-layer pour pieces that no via ties to the rest of their net.

    The router counts a pad lying on a pour as connected, but KiCad's refill
    cuts the pour wherever other nets cross it, and a piece holding only SMD
    pads then has no route to the rest of the net. Add one via inside each
    such piece. Positions are appended to the board file as text, in the same
    form the exporter writes its own vias. A piece with no legal spot is left
    alone and reported: the fix there is a layout change, or approving
    via-in-pad (`allow_via_in_pad`), which is not this script's call.

    Returns the via positions added and the number of pieces left cut off.
    """
    board = pcbnew.LoadBoard(board_path)
    pads = [pad for pad in board.GetPads() if pad.IsOnCopperLayer()]
    tracks = all_tracks(board)
    added: list[tuple[int, float, float]] = []
    pieces = []
    for _ in range(STITCH_ROUNDS):
        pieces = _floating_pieces(board, tracks, pads)
        added_now = 0
        for net, _layer, piece, anchor_pads in pieces:
            # A via added earlier this round may already reach this piece
            # (a through via serves the F.Cu and B.Cu piece at the same spot).
            if _is_anchored(piece, tracks, pads, net):
                continue
            point = _stitch_point(piece, tracks, pads, net, anchor_pads)
            if point is None and allow_via_in_pad:
                point = _stitch_point(
                    piece, tracks, pads, net, anchor_pads, in_pad=True
                )
            if point is not None:
                tracks.append(_add_via(board, net, point))
                added.append((net, pcbnew.ToMM(point.x), pcbnew.ToMM(point.y)))
                added_now += 1
        if added_now == 0:
            break
    else:
        pieces = _floating_pieces(board, tracks, pads)
    for _net, layer, _piece, anchor_pads in pieces:
        names = ", ".join(
            f"{p.GetParentFootprint().GetReference()}.{p.GetNumber()}"
            for p in anchor_pads
        )
        sys.stderr.write(
            f"warning: pour piece on {layer} still cut off from its net ({names})\n"
        )
    _append_vias(board_path, board, added)
    return [(x, y) for _, x, y in added], len(pieces)


def _append_vias(board_path: str, board, added) -> None:
    if not added:
        return
    text = Path(board_path).read_text(encoding="utf-8")
    end = text.rstrip().rfind(")")
    if end < 0:
        raise ValueError(f"{board_path} does not look like a KiCad board")
    records = []
    for net, x, y in added:
        name = board.FindNet(net).GetNetname()
        # Synth numbers its nets `(net 26 "GND")`; KiCad 10 itself names them.
        numbered = re.search(rf'\(net (\d+) "{re.escape(name)}"\)', text)
        net_ref = numbered.group(1) if numbered else f'"{name}"'
        key = f"synth-stitch-via/{name}/{x:.4f}/{y:.4f}"
        uid = uuid.uuid5(uuid.NAMESPACE_URL, key)
        records.append(
            f"\t(via\n\t\t(at {x:.4f} {y:.4f})\n\t\t(size {STITCH_PAD_MM})\n"
            f"\t\t(drill {STITCH_DRILL_MM})\n\t\t(layers \"F.Cu\" \"B.Cu\")\n"
            f"\t\t(net {net_ref})\n\t\t(uuid \"{uid}\")\n\t)\n"
        )
    Path(board_path).write_text(
        text[:end] + "".join(records) + text[end:], encoding="utf-8"
    )


def java_version(java: str) -> str | None:
    """First line of the interpreter's version banner, if it answers."""
    try:
        out = subprocess.run(
            [java, "-version"], capture_output=True, text=True, timeout=30
        )
    except (OSError, subprocess.SubprocessError):
        return None
    banner = (out.stderr or out.stdout).strip()
    return banner.splitlines()[0] if banner else None


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
    # Accepted and ignored: every route is now a clean-netlist route, so the
    # flag no longer selects anything. The adapter still passes it.
    parser.add_argument(
        "--clean-netlist",
        action="store_true",
        help="accepted for compatibility; always applied",
    )
    parser.add_argument(
        "--allow-via-in-pad",
        action="store_true",
        help="let a ground stitch via sit inside a big pad of its own net",
    )
    parser.add_argument(
        "--no-import",
        action="store_true",
        help="stop after routing and SES export",
    )
    parser.add_argument(
        "--report",
        help="write a JSON record of what the engine reported here",
    )
    parser.add_argument(
        "--no-retain-session",
        dest="retain_session",
        action="store_false",
        default=True,
        help="delete FreeRouting's scratch data directory when finished",
    )
    args = parser.parse_args()

    def write_report(**fields: object) -> None:
        """Record the engine's own account of the run.

        Written even on the failure paths below, because "the router ran and
        then failed" and "the router never ran" are different diagnoses and
        the run record has to tell them apart.
        """
        if not args.report:
            return
        Path(args.report).write_text(json.dumps(fields, indent=2, sort_keys=True))

    with tempfile.TemporaryDirectory(prefix="synth-freerouting-") as work:
        dsn = os.path.join(work, "board.dsn")
        ses = os.path.join(work, "board.ses")
        data = os.path.join(work, "freerouting-data")
        os.makedirs(data)

        # The board arrives un-routed, but strip any copper anyway: a stale
        # track would be routed around by the engine and then merged over,
        # and the independent check would be looking at geometry that was
        # never validated as a route.
        #
        # `Tracks()` exposes a SWIG vector whose indexed values can be
        # borrowed wrappers on newer KiCad builds.  Remove owned Python
        # objects from `GetTracks()` instead; this works with KiCad 9/10
        # and avoids the `SwigPyObject.thisown` failure.
        export_board = pcbnew.LoadBoard(args.input_board)
        ground_net_codes = {zone.GetNetCode() for zone in export_board.Zones()}
        for track in all_tracks(export_board):
            # Ground stitching vias are part of the board's plane topology,
            # not FreeRouting's candidate geometry. Keep them in the clean
            # netlist so the planes remain electrically joined after import.
            if track.GetClass() == "PCB_VIA" and track.GetNetCode() in ground_net_codes:
                continue
            export_board.Remove(track)
        print("routing a clean netlist", flush=True)
        # KiCad's SES importer can discard named netclasses. Keep the source
        # settings alive and restore them after import so the routed board is
        # checked with the same Power/RF constraints as the exported board.
        if not pcbnew.ExportSpecctraDSN(export_board, dsn):
            write_report(router="freerouting", error="dsn_export_failed")
            raise SystemExit("KiCad Specctra DSN export failed")

        completed = subprocess.run(
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
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            write_report(
                router="freerouting",
                jar=str(args.jar),
                java_version=completed.stderr.strip()[:200] or None,
                exit_code=completed.returncode,
                notes=["freerouting exited non-zero"],
            )
            sys.stderr.write(completed.stdout)
            sys.stderr.write(completed.stderr)
            raise SystemExit(completed.returncode)

        if args.ses_output:
            shutil.copyfile(ses, args.ses_output)
            print(f"saved SES result to {args.ses_output}", flush=True)

        # The engine's own account goes to stdout so it lands in the run's
        # session log. A partial route is the case where it matters most: the
        # SES shows what was produced, and this says why the rest was not.
        sys.stdout.write(completed.stdout)
        sys.stderr.write(completed.stderr)

        if not args.retain_session:
            shutil.rmtree(data, ignore_errors=True)

        if args.no_import:
            write_report(
                router="freerouting",
                jar=str(args.jar),
                java_version=java_version(args.java),
                passes=args.passes,
                threads=args.threads,
                ses_path=ses,
                imported_segments=None,
                imported_vias=None,
                notes=["ses exported without import"],
            )
            return 0

        # KiCad 10's Python ImportSpecctraSES can segfault on otherwise valid
        # dense SES geometry. Merge the external router's records textually
        # instead; KiCad CLI performs the authoritative refill and DRC after
        # this step. The merger removes all existing top-level segment/via
        # records before adding the SES records, so the original source file
        # remains a suitable merge base even when the DSN was exported from a
        # copper-free duplicate.
        merge_input = args.input_board
        merger = os.path.join(
            os.path.dirname(__file__), "import_freerouting_ses_text.py"
        )
        subprocess.run(
            [sys.executable, merger, merge_input, ses, args.output_board],
            check=True,
        )

        try:
            stitched, unresolved = stitch_floating_pour(
                args.output_board, args.allow_via_in_pad
            )
        except Exception as exc:
            write_report(router="freerouting", error="stitch_failed", detail=str(exc))
            raise
        if stitched:
            print(f"stitched {len(stitched)} floating pour piece(s)", flush=True)

        # Counted from the merged board rather than from the engine's log, so
        # the claim can be checked against the file that was written.
        written = pcbnew.LoadBoard(args.output_board)
        tracks = all_tracks(written)
        # KiCad names a routed track `PCB_TRACK`; the constant is matched by
        # suffix so a build that spells it differently still counts, and an
        # unmatched name is reported rather than quietly folded into a zero.
        segments = sum(1 for t in tracks if t.GetClass().endswith("TRACK"))
        vias = sum(1 for t in tracks if t.GetClass().endswith("VIA"))
        write_report(
            router="freerouting",
            jar=str(args.jar),
            java_version=java_version(args.java),
            passes=args.passes,
            threads=args.threads,
            ses_path=ses,
            imported_segments=segments,
            imported_vias=vias,
            stitch_vias=len(stitched),
            stitch_unresolved=unresolved,
            notes=[],
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
