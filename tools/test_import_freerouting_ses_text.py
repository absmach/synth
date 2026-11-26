#!/usr/bin/env python3
"""Run: python3 tools/test_import_freerouting_ses_text.py"""

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("import_freerouting_ses_text.py")
sys.path.insert(0, str(SCRIPT.parent))
import import_freerouting_ses_text as importer  # noqa: E402

BOARD = '(kicad_pcb\n\t(net 0 "")\n\t(net 1 "A")\n\t(net 2 "B")\n)\n'

ZONE = '(zone (net 0) (layer "F.Cu") (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))'

SES = """(session board.ses
  (routes
    (resolution um 10)
    (network_out
      (net A
        (wire (path F.Cu 1270 0 0 100000 0))
        (wire (path F.Cu 1270 0 0 100000 0))
        (via "Via[0-1]_600:300_um" 100000 0)
      )
      (net B
        (wire (path In1.Cu 2420 0 10000 100000 10000))
      )
    )
  )
)
"""


def run_importer(directory: Path, name: str, board_text: str = BOARD) -> str:
    board = directory / "board.kicad_pcb"
    ses = directory / "board.ses"
    out = directory / name
    board.write_text(board_text)
    ses.write_text(SES)
    subprocess.run(
        [sys.executable, str(SCRIPT), str(board), str(ses), str(out)],
        check=True,
        capture_output=True,
    )
    return out.read_text()


class NameOnlyNets(unittest.TestCase):
    def test_a_board_without_a_numeric_net_table_still_imports(self):
        # A previous textual merge, or a portfolio run whose baseline came
        # from a routed board, names nets on pads directly. The SES names the
        # same nets; the importer must resolve them instead of failing.
        board = (
            '(kicad_pcb\n\t(footprint "X:Y"\n'
            '\t\t(pad "1" smd circle (at 0 0) (size 1 1) (net "A"))\n'
            '\t\t(pad "2" smd circle (at 2 0) (size 1 1) (net "B"))\n\t)\n)\n'
        )
        with tempfile.TemporaryDirectory() as directory:
            text = run_importer(Path(directory), "out.kicad_pcb", board)
        self.assertIn('(net "A")', text)
        self.assertIn('(net "B")', text)


def items(node, name):
    return [x for x in node[1:] if isinstance(x, list) and x and x[0] == name]


class MissingNet(unittest.TestCase):
    def test_an_ses_net_absent_from_the_board_names_the_net(self):
        board = '(kicad_pcb\n\t(net 0 "")\n\t(net 1 "A")\n)\n'
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            board_path = directory / "board.kicad_pcb"
            ses_path = directory / "board.ses"
            out = directory / "out.kicad_pcb"
            board_path.write_text(board)
            ses_path.write_text(SES)
            result = subprocess.run(
                [sys.executable, str(SCRIPT), str(board_path), str(ses_path), str(out)],
                check=False,
                capture_output=True,
                text=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("B", result.stderr)
        self.assertIn("not in the board net table", result.stderr)


class DeterministicUuids(unittest.TestCase):
    def test_two_runs_are_byte_identical(self):
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            one = run_importer(Path(first), "a.kicad_pcb")
            two = run_importer(Path(second), "b.kicad_pcb")
        self.assertEqual(one, two)

    def test_every_item_gets_a_distinct_uuid(self):
        with tempfile.TemporaryDirectory() as directory:
            text = run_importer(Path(directory), "a.kicad_pcb")
        uuids = re.findall(r'\(uuid "([^"]+)"\)', text)
        self.assertEqual(len(uuids), 4)
        self.assertEqual(len(set(uuids)), 4)


class InsertionPoint(unittest.TestCase):
    """Imported routes must be children of the board root, not of a footprint."""

    def merge(self, board: str):
        with tempfile.TemporaryDirectory() as directory:
            text = run_importer(Path(directory), "out.kicad_pcb", board)
        return text, importer.parse(text)

    def assert_at_board_level(self, root) -> None:
        self.assertEqual(root[0], "kicad_pcb")
        self.assertEqual(len(items(root, "segment")), 3)
        self.assertEqual(len(items(root, "via")), 1)

    def test_zone_inside_footprint(self):
        board = (
            '(kicad_pcb\n\t(net 0 "")\n\t(net 1 "A")\n\t(net 2 "B")\n'
            '\t(footprint "U1"\n\t\t(layer "F.Cu")\n'
            '\t\t(property "Description" "a ) ( zone")\n'
            f"\t{ZONE}\n\t)\n)\n"
        )
        _text, root = self.merge(board)
        self.assert_at_board_level(root)
        self.assertEqual(len(items(root, "footprint")), 1)
        self.assertEqual(len(items(items(root, "footprint")[0], "zone")), 1)

    def test_top_level_zone(self):
        board = (
            '(kicad_pcb\n\t(net 0 "")\n\t(net 1 "A")\n\t(net 2 "B")\n'
            f"\t{ZONE}\n)\n"
        )
        text, root = self.merge(board)
        self.assert_at_board_level(root)
        order = [x[0] for x in root[1:] if isinstance(x, list)]
        self.assertEqual(order.count("zone"), 1)
        self.assertLess(order.index("segment"), order.index("zone"))
        # The zone keeps its leading tab and no whitespace-only line appears
        # before it; inserting at the zone's "(" drops both.
        lines = text.splitlines()
        zone_line = next(
            index for index, line in enumerate(lines) if line.lstrip().startswith("(zone")
        )
        self.assertTrue(lines[zone_line].startswith("\t(zone"))
        self.assertNotEqual(lines[zone_line - 1].strip(), "")

    def test_no_zone(self):
        _text, root = self.merge(BOARD)
        self.assert_at_board_level(root)


if __name__ == "__main__":
    unittest.main()
