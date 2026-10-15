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


def build_open_board(
    path, boxed=False, corridor=False, far_size=1.0, far_back=False, pour_net="GND"
):
    """A solid 30 mm ground pour on both layers, one via at (2, 2), and SMD
    ground pads at (3, 2) (1 mm from the via) and (22, 20) (far from any tie).
    With `boxed` the far pad is walled in by signal tracks, 1.5 mm across inside: no room for a via.
    `pour_net` names the net the pour, the pads and the via are on."""
    board = pcbnew.BOARD()
    gnd = pcbnew.NETINFO_ITEM(board, pour_net)
    sig = pcbnew.NETINFO_ITEM(board, "SIG")
    board.Add(gnd)
    board.Add(sig)
    for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
        zone = pcbnew.ZONE(board)
        zone.SetLayer(layer)
        zone.SetNet(gnd)
        zone.SetLocalClearance(mm(0.2))
        zone.SetPadConnection(pcbnew.ZONE_CONNECTION_FULL)
        outline = zone.Outline()
        outline.NewOutline()
        for x, y in ((0, 0), (30, 0), (30, 30), (0, 30)):
            outline.Append(mm(x), mm(y))
        board.Add(zone)
    if boxed:
        for (x1, y1), (x2, y2) in (
            ((21, 19), (23, 19)),
            ((21, 21), (23, 21)),
            ((21, 19), (21, 21)),
            ((23, 19), (23, 21)),
        ):
            wall = pcbnew.PCB_TRACK(board)
            wall.SetLayer(pcbnew.F_Cu)
            wall.SetNet(sig)
            wall.SetWidth(mm(0.5))
            wall.SetStart(point(x1, y1))
            wall.SetEnd(point(x2, y2))
            board.Add(wall)
    if corridor:
        # Walls 2.2 mm apart, 6 mm long: the pour between them is 1.3 mm wide
        # once the clearance is taken off, so a via fits there only with a small
        # margin, and the open pour past the walls is 3 mm from the pad.
        for x in (20.9, 23.1):
            wall = pcbnew.PCB_TRACK(board)
            wall.SetLayer(pcbnew.F_Cu)
            wall.SetNet(sig)
            wall.SetWidth(mm(0.5))
            wall.SetStart(point(x, 17))
            wall.SetEnd(point(x, 23))
            board.Add(wall)
    for ref, x, y in (("C1", 3, 2), ("C2", 22, 20)):
        footprint = pcbnew.FOOTPRINT(board)
        footprint.SetReference(ref)
        footprint.SetPosition(point(x, y))
        pad = pcbnew.PAD(footprint)
        pad.SetNumber("2")
        pad.SetShape(pcbnew.PAD_SHAPE_RECT)
        pad.SetAttribute(pcbnew.PAD_ATTRIB_SMD)
        back = far_back and ref == "C2"
        layers = pcbnew.LSET()
        if back:
            for layer in (pcbnew.B_Cu, pcbnew.B_Mask, pcbnew.B_Paste):
                layers.AddLayer(layer)
        pad.SetLayerSet(layers if back else pad.SMDMask())
        width, height = (
            (far_size if isinstance(far_size, tuple) else (far_size, far_size))
            if ref == "C2"
            else (1, 1)
        )
        pad.SetSize(point(width, height))
        pad.SetPosition(point(x, y))
        pad.SetNet(gnd)
        footprint.Add(pad)
        board.Add(footprint)
    tie = pcbnew.PCB_VIA(board)
    tie.SetPosition(point(2, 2))
    tie.SetViaType(pcbnew.VIATYPE_THROUGH)
    tie.SetWidth(pcbnew.F_Cu, mm(0.6))
    tie.SetDrill(mm(0.3))
    tie.SetNet(gnd)
    board.Add(tie)
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


@unittest.skipIf(pcbnew is None, "pcbnew not available")
class TieDistantGroundPads(unittest.TestCase):
    """A ground pad on the pour is not tied to the planes unless a via is near:
    the return current then runs through the pour. Every outer ground pad with
    no tie within `STITCH_TIE_MM` gets a via beside it."""

    def test_a_far_pad_gets_a_via_and_a_near_pad_does_not(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path)

            added, unresolved = router.tie_distant_ground_pads(str(path))

            self.assertEqual(len(added), 1)
            self.assertEqual(unresolved, 0)
            x, y = added[0]
            # Outside the 1 mm pad (and its clearance), within 2 mm of its edge.
            edge = max(abs(x - 22), abs(y - 20)) - 0.5
            self.assertGreaterEqual(edge, 0.2 + 0.3 - 1e-6)
            self.assertLessEqual(edge, 2.0)

    def test_ground_nets_by_any_common_name_are_tied(self):
        import freeroute_autoroute as router

        for name in ("GND", "AGND", "DGND", "GNDA", "/GND", "VSS"):
            with self.subTest(net=name), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "board.kicad_pcb"
                build_open_board(path, pour_net=name)
                added, _ = router.tie_distant_ground_pads(str(path))
                self.assertEqual(len(added), 1)

    def test_a_power_or_signal_pour_is_left_alone(self):
        import freeroute_autoroute as router

        for name in ("VBUS", "+3V3", "VCC", "SIG", "RXD1"):
            with self.subTest(net=name), tempfile.TemporaryDirectory() as tmp:
                path = Path(tmp) / "board.kicad_pcb"
                build_open_board(path, pour_net=name)
                before = path.read_bytes()
                self.assertEqual(router.tie_distant_ground_pads(str(path)), ([], 0))
                self.assertEqual(path.read_bytes(), before)

    def test_a_second_run_adds_nothing(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path)
            router.tie_distant_ground_pads(str(path))
            self.assertEqual(router.tie_distant_ground_pads(str(path)), ([], 0))

    def test_a_pad_with_no_room_is_reported_not_forced(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path, boxed=True)
            added, unresolved = router.tie_distant_ground_pads(str(path))
            self.assertEqual(added, [])
            self.assertEqual(unresolved, 1)

    def test_same_input_gives_the_same_bytes(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "source.kicad_pcb"
            build_open_board(source)
            outputs = []
            for name in ("a", "b"):
                path = Path(tmp) / f"{name}.kicad_pcb"
                path.write_bytes(source.read_bytes())
                router.tie_distant_ground_pads(str(path))
                outputs.append(path.read_bytes())
            self.assertEqual(outputs[0], outputs[1])

    def test_a_via_is_never_put_on_top_of_another(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path)
            board = pcbnew.LoadBoard(str(path))
            net = board.GetNetcodeFromNetname("GND")
            tracks = router.all_tracks(board)
            pads = [pad for pad in board.GetPads() if pad.IsOnCopperLayer()]
            # The tie at (2, 2) is a ground via: a new via at the same spot,
            # or touching it, must be refused.
            self.assertFalse(router._via_fits(tracks, pads, net, point(2, 2)))
            self.assertFalse(router._via_fits(tracks, pads, net, point(2.4, 2)))
            self.assertTrue(router._via_fits(tracks, pads, net, point(8, 8)))

    def test_a_via_farther_than_the_tie_distance_does_not_count(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path)
            old = router.STITCH_TIE_MM
            router.STITCH_TIE_MM = 0.4  # a via needs 0.5 mm from a pad's edge
            try:
                added, unresolved = router.tie_distant_ground_pads(str(path))
            finally:
                router.STITCH_TIE_MM = old
            self.assertEqual(added, [])
            self.assertEqual(unresolved, 2)

    def test_a_near_spot_with_little_margin_beats_a_far_one_with_more(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path, corridor=True)
            added, unresolved = router.tie_distant_ground_pads(str(path))
            self.assertEqual(unresolved, 0)
            self.assertEqual(len(added), 1)
            x, y = added[0]
            self.assertLessEqual(((x - 22) ** 2 + (y - 20) ** 2) ** 0.5, 2.0)

    def test_a_pad_on_the_back_layer_is_tied_too(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path, far_back=True)
            added, unresolved = router.tie_distant_ground_pads(str(path))
            self.assertEqual((len(added), unresolved), (1, 0))

    def test_a_large_pad_is_tied_by_a_via_at_its_edge(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            build_open_board(path, far_size=3.4)
            # No via fits within 2 mm of the pad's centre, but one fits beside
            # its edge, which is what counts.
            added, unresolved = router.tie_distant_ground_pads(str(path))
            self.assertEqual((len(added), unresolved), (1, 0))
            x, y = added[0]
            self.assertGreaterEqual(max(abs(x - 22), abs(y - 20)) - 1.7, 0.5 - 1e-6)
            # A second run sees the pad as tied.
            self.assertEqual(router.tie_distant_ground_pads(str(path)), ([], 0))

    def test_a_via_stays_out_of_the_disc_the_gate_reads_a_long_pad_as(self):
        import freeroute_autoroute as router

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "board.kicad_pcb"
            # A 2.0 x 0.6 mm pad. Synth's connectivity check treats a pad as the
            # disc of its larger half-dimension (1.0 mm here), and calls a via of
            # the same net inside it a via-in-pad, which costs a filled and
            # capped via at the fab. A via beside the pad but inside that disc
            # would be reported as one.
            build_open_board(path, far_size=(2.0, 0.6))
            added, unresolved = router.tie_distant_ground_pads(str(path))
            self.assertEqual((len(added), unresolved), (1, 0))
            x, y = added[0]
            self.assertGreater(((x - 22) ** 2 + (y - 20) ** 2) ** 0.5, 1.0)


if __name__ == "__main__":
    unittest.main()
