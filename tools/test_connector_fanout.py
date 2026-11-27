# SPDX-License-Identifier: Apache-2.0
"""Tests for the connector-dogbone escape (absmach/synth#433).

The fixture is a DF40-like connector: 2 x 20 pads at 0.4 mm pitch, 0.2 x 0.7
mm pads, rows 3.08 mm apart, every pad on its own net, on a six-layer board.
The clearances are re-derived here from the board the helper wrote, in board
coordinates, not taken from the helper's own check.
"""

from __future__ import annotations

import math
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HELPER = Path(__file__).with_name("connector_fanout.py")


STACK = """\t(layers
\t\t(0 "F.Cu" signal)
\t\t(4 "In1.Cu" signal)
\t\t(6 "In2.Cu" signal)
\t\t(8 "In3.Cu" signal)
\t\t(10 "In4.Cu" signal)
\t\t(2 "B.Cu" signal)
\t\t(25 "Edge.Cuts" user)
\t)"""


def connector_board(row_spacing: float = 3.08, angle: float = 90.0,
                    at: tuple[float, float] = (50.0, 40.0), extra: str = "",
                    stack: str = STACK) -> tuple[str, list[dict]]:
    """A board with one connector J1, plus `extra` board items; returns the
    text and J1's pads, absolute."""
    nets = ["\t(net 0 \"\")"]
    pads = []
    lines = []
    number = 1
    for row, y in ((0, -row_spacing / 2), (1, row_spacing / 2)):
        for i in range(20):
            x = (i - 9.5) * 0.4
            net = number
            nets.append(f'\t(net {net} "S{net}")')
            lines.append(
                f'\t\t(pad "{number}" smd rect (at {x:.4f} {y:.4f} {angle}) (size 0.2 0.7) '
                f'(layers "F.Cu" "F.Paste" "F.Mask") (net {net} "S{net}") (uuid "p{number}"))'
            )
            a = math.radians(angle)
            pads.append({
                "name": str(number), "net": net,
                "x": at[0] + x * math.cos(a) + y * math.sin(a),
                "y": at[1] - x * math.sin(a) + y * math.cos(a),
                # A 90 degree footprint turns the 0.2 x 0.7 pad on its side.
                "w": 0.7 if round(angle / 90) % 2 else 0.2,
                "h": 0.2 if round(angle / 90) % 2 else 0.7,
            })
            number += 1
    nets.append('\t(net 999 "OTHER")')
    text = "(kicad_pcb (version 20240108) (generator \"test\")\n" + stack + "\n" + "\n".join(nets) + (
        f'\n\t(footprint "Conn:DF40" (layer "F.Cu") (uuid "fp") (at {at[0]} {at[1]} {angle})\n'
        '\t\t(property "Reference" "J1" (at 0 -3 0) (layer "F.SilkS") (uuid "r"))\n'
        + "\n".join(lines) + "\n\t)\n" + extra + "\n)\n"
    )
    return text, pads


def run(board: str, *extra: str, layers: tuple[str, ...] = ("F.Cu", "In1.Cu")
        ) -> tuple[subprocess.CompletedProcess, str | None]:
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "in.kicad_pcb"
        out = Path(tmp) / "out.kicad_pcb"
        src.write_text(board)
        done = subprocess.run(
            [sys.executable, str(HELPER), str(src), "--component", "J1", "--output", str(out),
             "--layers", *layers, *extra],
            capture_output=True, text=True, check=False,
        )
        return done, out.read_text() if out.is_file() else None


def vias(text: str) -> list[dict]:
    out = []
    for m in re.finditer(r'\(via (\w+) \(at ([-\d.]+) ([-\d.]+)\) \(size ([\d.]+)\) '
                         r'\(drill ([\d.]+)\) \(layers "([^"]+)" "([^"]+)"\) \(net (\d+)\)', text):
        out.append({"kind": m.group(1), "x": float(m.group(2)), "y": float(m.group(3)),
                    "size": float(m.group(4)), "drill": float(m.group(5)),
                    "layers": (m.group(6), m.group(7)), "net": int(m.group(8))})
    return out


def stubs(text: str) -> list[dict]:
    out = []
    for m in re.finditer(r'\(segment \(start ([-\d.]+) ([-\d.]+)\) \(end ([-\d.]+) ([-\d.]+)\) '
                         r'\(width ([\d.]+)\) \(layer "([^"]+)"\) \(net (\d+)\)', text):
        out.append({"a": (float(m.group(1)), float(m.group(2))),
                    "b": (float(m.group(3)), float(m.group(4))),
                    "width": float(m.group(5)), "layer": m.group(6), "net": int(m.group(7))})
    return out


def point_rect(px, py, pad) -> float:
    dx = max(abs(px - pad["x"]) - pad["w"] / 2, 0.0)
    dy = max(abs(py - pad["y"]) - pad["h"] / 2, 0.0)
    return math.hypot(dx, dy)


def point_segment(p, a, b) -> float:
    dx, dy = b[0] - a[0], b[1] - a[1]
    t = max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy or 1)))
    return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)


class DogboneTest(unittest.TestCase):
    def setUp(self):
        self.board, self.pads = connector_board()
        self.done, self.out = run(self.board)
        self.assertEqual(self.done.returncode, 0, self.done.stderr)
        self.vias = vias(self.out)
        self.stubs = stubs(self.out)

    def test_every_signal_pad_is_escaped_with_a_microvia_and_a_stub(self):
        self.assertEqual(len(self.vias), 40)
        self.assertEqual(len(self.stubs), 40)
        self.assertEqual({v["net"] for v in self.vias}, {p["net"] for p in self.pads})
        for via in self.vias:
            self.assertEqual((via["size"], via["drill"]), (0.25, 0.1))
            # A laser microvia spans one layer pair: F.Cu to the layer under it.
            self.assertEqual(via["kind"], "micro")
            self.assertEqual(via["layers"], ("F.Cu", "In1.Cu"))
        for stub in self.stubs:
            self.assertEqual((stub["width"], stub["layer"]), (0.15, "F.Cu"))
        self.assertIn('"escaped": 40', self.done.stdout)
        self.assertIn('"failed": 0', self.done.stdout)

    def test_no_via_drill_intersects_an_smd_pad(self):
        for via in self.vias:
            for pad in self.pads:
                self.assertGreater(point_rect(via["x"], via["y"], pad), via["drill"] / 2,
                                   f"via of net {via['net']} drills pad {pad['name']}")

    def test_clearances_between_nets_are_at_least_a_tenth_of_a_millimetre(self):
        floor = 0.1 - 1e-6
        for via in self.vias:
            r = via["size"] / 2
            for pad in self.pads:
                if pad["net"] != via["net"]:
                    self.assertGreaterEqual(point_rect(via["x"], via["y"], pad) - r, floor)
            for other in self.vias:
                if other["net"] != via["net"]:
                    gap = math.dist((via["x"], via["y"]), (other["x"], other["y"])) - 2 * r
                    self.assertGreaterEqual(gap, floor)
            for stub in self.stubs:
                if stub["net"] != via["net"]:
                    gap = point_segment((via["x"], via["y"]), stub["a"], stub["b"]) - r - stub["width"] / 2
                    self.assertGreaterEqual(gap, floor, f"via {via['net']} to stub {stub['net']}")

    def test_vias_sit_in_the_channel_between_the_rows(self):
        # At 90 degrees the rows run along y, at x = 50 -+ 1.54.
        for via in self.vias:
            self.assertGreater(via["x"], 50 - 1.54 + 0.35)
            self.assertLess(via["x"], 50 + 1.54 - 0.35)

    def test_the_grid_is_staggered(self):
        depths = {round(abs(via["x"] - 50.0), 3) for via in self.vias}
        self.assertEqual(len(depths), 2, depths)


def other_part(x: float, y: float, kind: str = "smd", size: float = 0.3) -> str:
    """A one-pad footprint R9 of net OTHER at board (x, y)."""
    layers = '"*.Cu"' if kind != "smd" else '"F.Cu" "F.Mask"'
    drill = f" (drill {size})" if kind != "smd" else ""
    net = ' (net 999 "OTHER")' if kind != "np_thru_hole" else ""
    return (f'\t(footprint "Test:Pad" (layer "F.Cu") (uuid "o") (at {x} {y} 0)\n'
            '\t\t(property "Reference" "R9" (at 0 0 0) (layer "F.SilkS") (uuid "o9"))\n'
            f'\t\t(pad "1" {kind} rect (at 0 0 0) (size {size} {size}){drill} (layers {layers}){net} (uuid "op"))\n'
            '\t)')


class BoardClearanceTest(unittest.TestCase):
    """The vias and stubs keep their clearance to the rest of the board, not
    only to the connector's own pads (absmach/synth#524 review)."""

    def assert_blocked(self, extra: str, what: str):
        board, _ = connector_board(extra=extra)
        done, out = run(board)
        self.assertEqual(done.returncode, 4, done.stdout)
        self.assertIsNone(out, "no board is written for a failed escape")
        self.assertRegex(done.stderr, rf"cannot escape J1 pad \d+: (via|stub) [\d.]+ mm from {what}")

    def test_a_neighbouring_part_pad_in_the_channel_blocks_the_escape(self):
        # The inner via line sits at x = 49.385 at 90 degrees.
        self.assert_blocked(other_part(49.6, 40.0), r"pad R9\.1")

    def test_a_hole_in_the_channel_blocks_the_escape(self):
        self.assert_blocked(other_part(49.6, 38.0, kind="np_thru_hole", size=0.5), "hole R9")

    def test_a_plated_hole_blocks_the_escape_on_every_layer(self):
        self.assert_blocked(other_part(49.6, 38.0, kind="thru_hole", size=0.4), r"pad R9\.1")

    def test_a_track_of_another_net_blocks_the_escape(self):
        track = ('\t(segment (start 49.385 30) (end 49.385 50) (width 0.15) (layer "F.Cu") '
                 '(net 999) (uuid "t"))')
        self.assert_blocked(track, "track")

    def test_a_track_on_an_untouched_layer_does_not_block(self):
        track = ('\t(segment (start 49.385 30) (end 49.385 50) (width 0.15) (layer "In3.Cu") '
                 '(net 999) (uuid "t"))')
        board, _ = connector_board(extra=track)
        done, _ = run(board)
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_the_board_edge_blocks_the_escape(self):
        # The last pads end at y = 43.9; an edge 0.2 mm past them leaves the
        # end vias inside the 0.3 mm copper-to-edge clearance.
        edge = '\t(gr_rect (start 30 30) (end 70 44.1) (layer "Edge.Cuts") (uuid "e"))'
        self.assert_blocked(edge, "board edge")

    def test_a_clear_board_outline_does_not_block(self):
        edge = '\t(gr_rect (start 30 30) (end 70 50) (layer "Edge.Cuts") (uuid "e"))'
        board, _ = connector_board(extra=edge)
        done, _ = run(board)
        self.assertEqual(done.returncode, 0, done.stderr)


class LayerTest(unittest.TestCase):
    def test_layers_without_the_adjacent_one_fail_instead_of_writing_a_blind_via(self):
        board, _ = connector_board()
        done, out = run(board, layers=("F.Cu", "In2.Cu", "In3.Cu"))
        self.assertEqual(done.returncode, 4)
        self.assertIsNone(out)
        self.assertIn("a microvia from F.Cu reaches In1.Cu", done.stderr)

    def test_a_bottom_connector_lands_on_the_last_inner_layer(self):
        board, _ = connector_board()
        board = board.replace('"F.Cu" "F.Paste" "F.Mask"', '"B.Cu" "B.Paste" "B.Mask"')
        done, out = run(board, layers=("B.Cu", "In4.Cu"))
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual({v["layers"] for v in vias(out)}, {("B.Cu", "In4.Cu")})


class FailureTest(unittest.TestCase):
    def test_an_unescapable_connector_fails_with_the_pad_name(self):
        # Rows 1.0 mm apart leave a 0.3 mm channel: no via fits.
        board, _ = connector_board(row_spacing=1.0)
        done, out = run(board)
        self.assertEqual(done.returncode, 4)
        self.assertIsNone(out, "no board is written for a failed escape")
        self.assertRegex(done.stderr, r"cannot escape J1 pad \d+")
        self.assertRegex(done.stdout, r'"failed": [1-9]')

    def test_a_missing_component_fails(self):
        board, _ = connector_board()
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "in.kicad_pcb"
            src.write_text(board)
            done = subprocess.run(
                [sys.executable, str(HELPER), str(src), "--component", "J9",
                 "--output", str(Path(tmp) / "o.kicad_pcb")],
                capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 4)
        self.assertIn("no footprint J9", done.stderr)


if __name__ == "__main__":
    unittest.main()
