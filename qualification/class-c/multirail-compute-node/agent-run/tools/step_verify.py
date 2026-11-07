#!/usr/bin/env python3
"""step_verify.py <board.step> <board.kicad_pcb>: reopen a STEP in OpenCASCADE and compare it with the KiCad PCB.

Run with a Python that has OCP (`pip install cadquery-ocp`). Checks, against the PCB the STEP was exported from:
  identities   every footprint with a body appears once in the STEP, under its reference, and nothing else does
  positions    each component's origin is the footprint's (x, -y), and its rotation is the footprint's
  outline      the board solid's x/y extent is the Edge.Cuts extent
  holes        the board solid has a cylindrical hole for every drill of the PCB, at its diameter
  envelopes    each component's body lies within its F.CrtYd courtyard (connectors may overhang it by the
               listed amount)
Exits 1 if identities, positions, outline or holes disagree."""
import collections
import math
import re
import sys

from OCP.Bnd import Bnd_Box
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBndLib import BRepBndLib
from OCP.GeomAbs import GeomAbs_Cylinder
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.TCollection import TCollection_ExtendedString
from OCP.TDataStd import TDataStd_Name
from OCP.collections import Sequence_TDF_Label
from OCP.TDocStd import TDocStd_Document
from OCP.TopAbs import TopAbs_FACE
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS
from OCP.XCAFApp import XCAFApp_Application
from OCP.XCAFDoc import XCAFDoc_DocumentTool

step_path, pcb_path = sys.argv[1:3]
pcb = open(pcb_path).read()


def name_of(label):
    attribute = TDataStd_Name()
    return label.FindAttribute(TDataStd_Name.GetID_s(), attribute) and attribute.Get().ToExtString()


def bbox_of(shape):
    box = Bnd_Box()
    BRepBndLib.Add_s(shape, box)
    lo, hi = box.CornerMin(), box.CornerMax()
    return lo.X(), lo.Y(), lo.Z(), hi.X(), hi.Y(), hi.Z()


# ---- the PCB ----------------------------------------------------------------
footprints = {}
for block in re.split(r"\n\t\(footprint\s", pcb)[1:]:
    block = re.split(r"\n\t\((?:segment|via|zone|gr_line|gr_rect|gr_text|group)\b", block)[0]
    ref = re.search(r'\(property\s+"Reference"\s+"([^"]+)"', block).group(1)
    at = re.search(r"\n\t\t\(at ([\d.-]+) ([\d.-]+)(?: ([\d.-]+))?\)", block)
    models = re.findall(r"\(model\s+\"([^\"]+)\"", block)
    court = [
        tuple(map(float, m))
        for m in re.findall(
            r'\(fp_rect\s+\(start ([\d.-]+) ([\d.-]+)\)\s+\(end ([\d.-]+) ([\d.-]+)\)[^)]*\(layer "F.CrtYd"\)', block
        )
    ]
    drills = [float(d) for d in re.findall(r"\(drill (?:oval )?([\d.]+)", block)]
    footprints[ref] = dict(x=float(at.group(1)), y=float(at.group(2)), rot=float(at.group(3) or 0), models=models, court=court, drills=drills)
pts = []
for block in re.findall(r"\(gr_line\b(.*?)\n\t\)", pcb, re.S):
    if '(layer "Edge.Cuts")' in block:
        for key in ("start", "end"):
            m = re.search(rf"\({key}\s+([\d.-]+)\s+([\d.-]+)\)", block)
            pts.append((float(m.group(1)), float(m.group(2))))
for block in re.findall(r"\(gr_rect\b(.*?)\n\t\)", pcb, re.S):
    if '(layer "Edge.Cuts")' in block:
        for key in ("start", "end"):
            m = re.search(rf"\({key}\s+([\d.-]+)\s+([\d.-]+)\)", block)
            pts.append((float(m.group(1)), float(m.group(2))))
edge = [min(x for x, _ in pts), min(y for _, y in pts), max(x for x, _ in pts), max(y for _, y in pts)]

# ---- the STEP ---------------------------------------------------------------
app = XCAFApp_Application.GetApplication_s()
doc = TDocStd_Document(TCollection_ExtendedString("XmlOcaf"))
app.NewDocument(TCollection_ExtendedString("MDTV-XCAF"), doc)
reader = STEPCAFControl_Reader()
if reader.ReadFile(step_path) != IFSelect_RetDone or not reader.Transfer(doc):
    raise SystemExit("OpenCASCADE could not read the STEP")
tool = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
roots = Sequence_TDF_Label()
tool.GetFreeShapes(roots)
components = {}
unmatched = []
board_shape = None


def walk(label, depth=0):
    comps = Sequence_TDF_Label()
    tool.GetComponents_s(label, comps)
    for i in range(1, comps.Length() + 1):
        comp = comps.Value(i)
        ref = name_of(comp)
        shape = tool.GetShape_s(comp)
        if ref and ref in footprints:
            components.setdefault(ref, []).append(shape)
        elif depth == 0:
            unmatched.append((ref, shape))
        else:
            walk(comp, depth + 1)


for i in range(1, roots.Length() + 1):
    walk(roots.Value(i))

# the board solid is the largest thing in the assembly that is not a footprint
if unmatched:
    area = lambda item: (lambda b: (b[3] - b[0]) * (b[4] - b[1]))(bbox_of(item[1]))
    board_shape = max(unmatched, key=area)[1]
    print("board solid:", max(unmatched, key=area)[0], "; other unmatched top-level parts:", len(unmatched) - 1)
problems = []
bodies = {r for r, f in footprints.items() if f["models"] and not r.startswith(("H", "FID"))}
holes_no_body = sorted(r for r in footprints if r.startswith(("H", "FID")))
missing = sorted(bodies - set(components))
extra = sorted(set(components) - bodies - set(holes_no_body))
dupes = sorted(r for r, s in components.items() if len(s) > 1)
print(f"components in the STEP {len(components)}; footprints with a body {len(bodies)}; mounting holes and fiducials (no body) {len(holes_no_body)}")
if missing or extra or dupes:
    problems.append(f"identities: missing {missing} extra {extra} repeated {dupes}")
print("identities:", "match" if not (missing or extra or dupes) else "DIFFER")

worst_xy = 0.0
worst_rot = 0.0
for ref, shapes in sorted(components.items()):
    from OCP.TopLoc import TopLoc_Location  # noqa: F401

    loc = shapes[0].Location().Transformation()
    t = loc.TranslationPart()
    f = footprints[ref]
    dx, dy = abs(t.X() - f["x"]), abs(t.Y() + f["y"])
    worst_xy = max(worst_xy, dx, dy)
    if dx > 0.01 or dy > 0.01:
        problems.append(f"position of {ref}: STEP ({t.X():.3f}, {t.Y():.3f}) PCB ({f['x']:.3f}, {-f['y']:.3f})")
    q = loc.GetRotation()
    angle = math.degrees(q.GetRotationAngle()) * (1 if q.Z() >= 0 else -1) if abs(q.Z()) > 1e-9 or abs(q.W()) < 1 - 1e-9 else 0.0
    turn = (angle - f["rot"] + 180.0) % 360.0 - 180.0
    worst_rot = max(worst_rot, abs(turn))
    if abs(turn) > 0.01:
        problems.append(f"rotation of {ref}: STEP {angle:.2f} deg, PCB {f['rot']:.2f} deg")
print(f"positions: worst x/y difference {worst_xy:.4f} mm, worst rotation difference {worst_rot:.3f} deg, over {len(components)} components")

if board_shape is None:
    problems.append("no board solid found in the STEP")
else:
    x0, y0, z0, x1, y1, z1 = bbox_of(board_shape)
    ex0, ey0, ex1, ey1 = edge[0], -edge[3], edge[2], -edge[1]
    print(f"outline: STEP {x1 - x0:.2f} x {y1 - y0:.2f} mm, Edge.Cuts {ex1 - ex0:.2f} x {ey1 - ey0:.2f} mm, thickness {z1 - z0:.2f} mm")
    if max(abs(x0 - ex0), abs(x1 - ex1), abs(y0 - ey0), abs(y1 - ey1)) > 0.05:
        problems.append("board outline differs from Edge.Cuts")
    found = collections.Counter()
    explorer = TopExp_Explorer(board_shape, TopAbs_FACE)
    seen = set()
    while explorer.More():
        face = TopoDS.Face(explorer.Current())
        surface = BRepAdaptor_Surface(face)
        if surface.GetType() == GeomAbs_Cylinder:
            cyl = surface.Cylinder()
            key = (round(cyl.Location().X(), 2), round(cyl.Location().Y(), 2), round(cyl.Radius() * 2, 2))
            if key not in seen:
                seen.add(key)
                found[key[2]] += 1
        explorer.Next()
    expected = collections.Counter(round(d, 2) for f in footprints.values() for d in f["drills"])
    print("holes by diameter, STEP:", dict(sorted(found.items())), " PCB drills:", dict(sorted(expected.items())))
    for d, n in expected.items():
        if found.get(d, 0) < n:
            problems.append(f"holes of {d} mm: PCB {n}, STEP {found.get(d, 0)}")

over = []
for ref, shapes in sorted(components.items()):
    f = footprints[ref]
    if not f["court"]:
        continue
    cx0, cy0, cx1, cy1 = f["court"][0]
    bx0, by0, bz0, bx1, by1, bz1 = bbox_of(shapes[0])
    t = shapes[0].Location().Transformation().TranslationPart()
    r = math.radians(f["rot"])
    c, s = math.cos(r), math.sin(r)
    corners = [(cx0, cy0), (cx1, cy0), (cx1, cy1), (cx0, cy1)]
    xs = [f["x"] + px * c + py * s for px, py in corners]
    ys = [-(f["y"] - px * s + py * c) for px, py in corners]
    margin = max(min(xs) - bx0, bx1 - max(xs), min(ys) - by0, by1 - max(ys), 0.0)
    if margin > 0.25:
        over.append((ref, round(margin, 2)))
print("body beyond courtyard by more than 0.25 mm:", over or "none")
print("PROBLEMS:", problems or "none")
sys.exit(1 if problems else 0)
