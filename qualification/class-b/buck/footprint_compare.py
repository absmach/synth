"""Compare each footprint on a board with its KiCad library original.
Usage: python3 -I footprint_compare.py buck.kicad_pcb [/usr/share/kicad/footprints]
Compares pads (number, type, shape, position, size, drill, layers, roundrect ratio),
courtyard, Fab, Mask and Paste graphics. Silkscreen, text and 3D models are ignored
and reported only as "silkscreen differs". Needs the pcbnew module that ships with KiCad.
Exit 1 if any footprint differs outside silkscreen."""
import os
import sys

import pcbnew

KEEP = [getattr(pcbnew, f"{s}_{n}") for s in "FB" for n in ("CrtYd", "Fab", "Mask", "Paste")]
SILK = [pcbnew.F_SilkS, pcbnew.B_SilkS]
R = lambda v: None if v is None else round(v / 1e6, 4)  # nm -> mm; None = unset margin
P = lambda p: (R(p.x), R(p.y))


def norm(fp):
    """Footprint copy at the origin, unrotated, on F.Cu, so only its own geometry is left."""
    fp = pcbnew.FOOTPRINT(fp)
    if fp.GetLayer() == pcbnew.B_Cu:
        fp.Flip(fp.GetPosition(), False)
    fp.SetOrientationDegrees(0)
    fp.SetPosition(pcbnew.VECTOR2I(0, 0))
    return fp


def pads(fp):
    out = {}
    for p in fp.Pads():
        d = p.GetDrillSize()
        out.setdefault(p.GetNumber(), []).append((
            p.GetAttribute(), p.GetShape(pcbnew.F_Cu), P(p.GetPosition()), P(p.GetSize(pcbnew.F_Cu)),
            P(d), P(p.GetOffset(pcbnew.F_Cu)), round(p.GetOrientationDegrees(), 3),
            round(p.GetRoundRectRadiusRatio(pcbnew.F_Cu), 4), tuple(sorted(p.GetLayerSet().Seq())),
            R(p.GetLocalSolderMaskMargin()), R(p.GetLocalSolderPasteMargin())))
    return {k: sorted(v) for k, v in out.items()}


def shape_key(g):
    pts = tuple(P(q) for q in g.GetCorners()) if g.GetShape() == pcbnew.SHAPE_T_POLY and hasattr(g, "GetCorners") else ()
    return (g.GetShape(), P(g.GetStart()), P(g.GetEnd()), P(g.GetCenter()), R(g.GetWidth()), g.IsSolidFill(), pts)


def graphics(fp, silk):
    out = {}
    for g in fp.GraphicalItems():
        if not isinstance(g, pcbnew.PCB_SHAPE):
            continue
        if g.GetLayer() not in (SILK if silk else KEEP):
            continue
        out.setdefault(g.GetLayerName(), []).append(shape_key(g))
    return {k: sorted(v) for k, v in out.items()}


def diff(a, b, label):
    return [f"{label} {k}: board={a.get(k)} library={b.get(k)}" for k in sorted(set(a) | set(b)) if a.get(k) != b.get(k)]


def main():
    board = pcbnew.LoadBoard(sys.argv[1])
    root = sys.argv[2] if len(sys.argv) > 2 else "/usr/share/kicad/footprints"
    bad = 0
    for fp in sorted(board.GetFootprints(), key=lambda f: f.GetReference()):
        lib, name = fp.GetFPIDAsString().split(":")
        ref = pcbnew.FootprintLoad(os.path.join(root, lib + ".pretty"), name)
        if ref is None:
            print(f"{fp.GetReference():8} {lib}:{name}: NOT FOUND in library")
            bad += 1
            continue
        a, b = norm(fp), norm(ref)
        issues = diff(pads(a), pads(b), "pad") + diff(graphics(a, False), graphics(b, False), "graphics")
        silk = graphics(a, True) != graphics(b, True)
        verdict = "DIFFERS (non-silkscreen)" if issues else ("only silkscreen differs" if silk else "identical")
        print(f"{fp.GetReference():8} {lib}:{name}: {verdict}")
        for i in issues:
            print("    " + i)
        bad += bool(issues)
    print(f"\n{bad} footprint(s) differ outside silkscreen")
    sys.exit(1 if bad else 0)


main()
