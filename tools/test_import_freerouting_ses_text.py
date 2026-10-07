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


def run_importer(directory: Path, name: str) -> str:
    board = directory / "board.kicad_pcb"
    ses = directory / "board.ses"
    out = directory / name
    board.write_text(BOARD)
    ses.write_text(SES)
    subprocess.run(
        [sys.executable, str(SCRIPT), str(board), str(ses), str(out)],
        check=True,
        capture_output=True,
    )
    return out.read_text()


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
