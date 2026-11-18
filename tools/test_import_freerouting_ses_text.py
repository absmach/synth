#!/usr/bin/env python3
"""Run: python3 tools/test_import_freerouting_ses_text.py"""

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("import_freerouting_ses_text.py")

BOARD = '(kicad_pcb\n\t(net 0 "")\n\t(net 1 "A")\n\t(net 2 "B")\n)\n'

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


if __name__ == "__main__":
    unittest.main()
