#!/usr/bin/env python3
"""Run: python3 tools/test_freeroute_stitch.py

A signal track cuts a ground pour in two. The half holding only an SMD ground
pad has no way to the rest of the net until `stitch_floating_pour` adds a via.
"""

import sys
import tempfile
import unittest
from pathlib import Path

try:
    import pcbnew
except ImportError:  # KiCad's Python bindings are optional on dev machines
    pcbnew = None

sys.path.insert(0, str(Path(__file__).parent))


def mm(value):
    return pcbnew.FromMM(value)


def point(x, y):
    return pcbnew.VECTOR2I(mm(x), mm(y))


def build_board(path, boxed_pad=False):
    board = pcbnew.BOARD()
    gnd = pcbnew.NETINFO_ITEM(board, "GND")
    sig = pcbnew.NETINFO_ITEM(board, "SIG")
    board.Add(gnd)
    board.Add(sig)

    for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
        zone = pcbnew.ZONE(board)
        zone.SetLayer(layer)
        zone.SetNet(gnd)
        zone.SetLocalClearance(mm(0.2))
        # As Synth exports it: `connect_pads yes`, a solid join.
        zone.SetPadConnection(pcbnew.ZONE_CONNECTION_FULL)
        outline = zone.Outline()
        outline.NewOutline()
        for x, y in ((0, 0), (20, 0), (20, 20), (0, 20)):
            outline.Append(mm(x), mm(y))
        board.Add(zone)

    wall = pcbnew.PCB_TRACK(board)
    wall.SetLayer(pcbnew.F_Cu)
    wall.SetNet(sig)
    wall.SetWidth(mm(0.5))
    wall.SetStart(point(10, -1))
    wall.SetEnd(point(10, 21))
    board.Add(wall)

    if boxed_pad:
        # A closed 2.1 mm box of pour filled almost edge to edge by a 1.8 mm pad
        # (like a QFN exposed pad): no room beside it for a via.
        for (x1, y1), (x2, y2) in (
            ((10, 8.5), (13, 8.5)),
            ((10, 11.5), (13, 11.5)),
            ((10, 8.5), (10, 11.5)),
            ((13, 8.5), (13, 11.5)),
        ):
            box = pcbnew.PCB_TRACK(board)
            box.SetLayer(pcbnew.F_Cu)
            box.SetNet(sig)
            box.SetWidth(mm(0.5))
            box.SetStart(point(x1, y1))
            box.SetEnd(point(x2, y2))
            board.Add(box)

    # Left of the wall: an SMD ground pad and a via down to the B.Cu plane.
    # Right of the wall: an SMD ground pad and nothing else.
    for ref, x in (("L1", 4), ("R1", 11.5 if boxed_pad else 16)):
        footprint = pcbnew.FOOTPRINT(board)
        footprint.SetReference(ref)
        footprint.SetPosition(point(x, 10))
        pad = pcbnew.PAD(footprint)
        pad.SetNumber("1")
        pad.SetShape(pcbnew.PAD_SHAPE_RECT)
        pad.SetAttribute(pcbnew.PAD_ATTRIB_SMD)
        pad.SetLayerSet(pad.SMDMask())
        pad.SetSize(point(1.8, 1.8) if boxed_pad and ref == "R1" else point(1, 1))
        pad.SetPosition(point(x, 10))
        pad.SetNet(gnd)
        footprint.Add(pad)
        board.Add(footprint)

    anchor = pcbnew.PCB_VIA(board)
    anchor.SetPosition(point(4, 14))
    anchor.SetViaType(pcbnew.VIATYPE_THROUGH)
    anchor.SetWidth(pcbnew.F_Cu, mm(0.6))
    anchor.SetDrill(mm(0.3))
    anchor.SetNet(gnd)
    board.Add(anchor)

    pcbnew.SaveBoard(str(path), board)


def unconnected(path):
    board = pcbnew.LoadBoard(str(path))
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
    connectivity = board.GetConnectivity()
    connectivity.RecalculateRatsnest()
    return connectivity.GetUnconnectedCount(True)


@unittest.skipIf(pcbnew is None, "pcbnew not available")
class StitchFloatingPour(unittest.TestCase):
    def test_cut_off_ground_piece_gets_one_via(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_board(path)
            self.assertGreater(unconnected(path), 0)

            added, unresolved = router.stitch_floating_pour(str(path))

            self.assertEqual(len(added), 1)
            self.assertEqual(unresolved, 0)
            self.assertGreater(added[0][0], 10)  # inside the right-hand piece
            self.assertEqual(unconnected(path), 0)

    def test_big_pad_takes_the_via_only_when_approved(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_board(path, boxed_pad=True)

            self.assertEqual(router.stitch_floating_pour(str(path)), ([], 1))
            self.assertGreater(unconnected(path), 0)

            added, unresolved = router.stitch_floating_pour(
                str(path), allow_via_in_pad=True
            )

            self.assertEqual(len(added), 1)
            self.assertEqual(unresolved, 0)
            self.assertEqual(unconnected(path), 0)

    def test_same_input_gives_the_same_bytes(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "source.kicad_pcb"
            build_board(source)  # pcbnew gives new items random ids: build once
            outputs = []
            for name in ("a", "b"):
                path = Path(tmp) / f"{name}.kicad_pcb"
                path.write_bytes(source.read_bytes())
                router.stitch_floating_pour(str(path))
                outputs.append(path.read_bytes())
            self.assertEqual(outputs[0], outputs[1])

    def test_numbered_nets_are_written_the_way_synth_numbers_them(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_board(path)
            # Synth's exporter writes `(net 7 "GND")`; KiCad 10 writes names.
            text = path.read_text().replace(
                '\t(footprint', '\t(net 7 "GND")\n\t(footprint', 1
            )
            path.write_text(text)

            router.stitch_floating_pour(str(path))

            self.assertIn("(net 7)", path.read_text())


if __name__ == "__main__":
    unittest.main()
