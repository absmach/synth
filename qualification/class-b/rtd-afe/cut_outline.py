#!/usr/bin/env python3
"""Cut the unused top-left notch out of the exported rectangle.

SynthSpec has no non-rectangular outline primitive, so the acceptance build
cuts the empty notch after export and refills the zones. The cut is a rectangle
open to the top edge: x in [cut_x0, cut_x1], y below cut_y. The left tab keeps
H1 and the top-left ground fence via.
"""

import sys

import pcbnew


def main() -> int:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: cut_outline.py <board.kicad_pcb> <cut_x0_mm> <cut_x1_mm> <cut_y_mm>"
        )
    board_path = sys.argv[1]
    cut_x0 = float(sys.argv[2])
    cut_x1 = float(sys.argv[3])
    cut_y = float(sys.argv[4])

    board = pcbnew.LoadBoard(board_path)
    box = board.GetBoardEdgesBoundingBox()
    x0, y0 = box.GetX() / 1e6, box.GetY() / 1e6
    x1, y1 = box.GetRight() / 1e6, box.GetBottom() / 1e6
    if not (x0 < cut_x0 < cut_x1 < x1 and y0 < cut_y < y1):
        raise SystemExit("cut coordinates are outside the exported outline")

    # Capture the drawing and copper lists before the outline edit: pcbnew's
    # wrapper invalidates one collection after the other is touched.
    drawings = list(board.GetDrawings())
    tracks = list(board.GetTracks())

    for drawing in drawings:
        if drawing.GetLayer() == pcbnew.Edge_Cuts:
            board.Remove(drawing)

    def add_line(xa: float, ya: float, xb: float, yb: float) -> None:
        shape = pcbnew.PCB_SHAPE(board)
        shape.SetShape(pcbnew.SHAPE_T_SEGMENT)
        shape.SetStart(pcbnew.VECTOR2I_MM(xa, ya))
        shape.SetEnd(pcbnew.VECTOR2I_MM(xb, yb))
        shape.SetLayer(pcbnew.Edge_Cuts)
        shape.SetWidth(pcbnew.FromMM(0.1))
        board.Add(shape)

    # Top edge: left tab, down into the notch, across its floor, back up to the
    # main top edge, then the right/bottom/left sides.
    points = [
        (x0, y0),
        (cut_x0, y0),
        (cut_x0, cut_y),
        (cut_x1, cut_y),
        (cut_x1, y0),
        (x1, y0),
        (x1, y1),
        (x0, y1),
    ]
    for i, (xa, ya) in enumerate(points):
        xb, yb = points[(i + 1) % len(points)]
        add_line(xa, ya, xb, yb)

    # The notch held only ground pour; drop any copper that fell inside it.
    for track in tracks:
        position = track.GetPosition()
        x, y = position.x / 1e6, position.y / 1e6
        if cut_x0 < x < cut_x1 and y < cut_y:
            board.Remove(track)

    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
    board.Save(board_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
