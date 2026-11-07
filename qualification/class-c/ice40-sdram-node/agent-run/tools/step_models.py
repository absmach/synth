#!/usr/bin/env python3
"""step_models.py <out dir> [3D library dir]: write the 3D models the board needs and KiCad's stock library lacks.

Run with a Python that has OCP (OpenCASCADE), for example `pip install cadquery-ocp` in a venv. The 3D library dir
defaults to the user flatpak's KiCad 3D library. Each model is in millimetres with its origin at the footprint origin
and +z up from the board surface:

  TestPoint_Pad_D1.5mm.step    the footprint's 1.5 mm pad as 1 oz copper: a disc 0.035 mm thick.
  Fuse_1812_4532Metric.step    the Bourns MF-MSMF110/16-2 at the middle of its datasheet ranges (MF-MSMF series datasheet,
                               Product Dimensions, Style 1 row of MF-MSMF110/16): 4.55 mm long (A 4.37 to 4.73), 3.24 mm
                               wide (B 3.07 to 3.41), 0.70 mm thick (C 0.55 to 0.85), one solid (the table gives no
                               termination length for this style).
  SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm.step
                               the library's SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.81mm body and leads, with the exposed
                               pad cut from 3.81 to the footprint's 3.3 mm (the pad is the 0.05 mm slab under the body).
  Broadcom_APDS-9301.step      (irrigation controller) the APDS-9301's miniature ChipLED package, 2.60 x 2.20 mm and
                               0.55 mm high, from its datasheet.

A mounting hole has no body: the hole is drilled in the board solid of the STEP.
"""
import sys
from pathlib import Path

from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut, BRepAlgoAPI_Fuse
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder
from OCP.BRep import BRep_Builder
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt
from OCP.TopoDS import TopoDS_Compound
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Reader, STEPControl_Writer


def box(x0, y0, z0, x1, y1, z1):
    return BRepPrimAPI_MakeBox(gp_Pnt(x0, y0, z0), gp_Pnt(x1, y1, z1)).Shape()


def disc(radius, height):
    return BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(0, 0, 0), gp_Dir(0, 0, 1)), radius, height).Shape()


def fuse_all(shapes):
    result = shapes[0]
    for shape in shapes[1:]:
        result = BRepAlgoAPI_Fuse(result, shape).Shape()
    return result


def write(shape, path):
    writer = STEPControl_Writer()
    writer.Transfer(shape, STEPControl_AsIs)
    if writer.Write(str(path)) != IFSelect_RetDone:
        raise SystemExit(f"could not write {path}")


out = Path(sys.argv[1])
library = Path(
    sys.argv[2]
    if len(sys.argv) > 2
    else Path.home() / ".local/share/flatpak/runtime/org.kicad.KiCad.Library.Packages3D/x86_64/stable/active/files/3dmodels"
)
out.mkdir(parents=True, exist_ok=True)
write(disc(0.75, 0.035), out / "TestPoint_Pad_D1.5mm.step")
builder = BRep_Builder()
fuse = TopoDS_Compound()
builder.MakeCompound(fuse)
builder.Add(fuse, box(-2.275, -1.62, 0.0, 2.275, 1.62, 0.70))
write(fuse, out / "Fuse_1812_4532Metric.step")

reader = STEPControl_Reader()
reader.ReadFile(str(library / "Package_SO.3dshapes/SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.81mm.step"))
reader.TransferRoots()
soic = reader.OneShape()
# The exposed pad is the slab under the body, z 0 to 0.05, 2.41 x 3.81 mm; cut it to 3.3 mm.
for y0, y1 in ((1.65, 2.0), (-2.0, -1.65)):
    soic = BRepAlgoAPI_Cut(soic, box(-1.3, y0, -0.01, 1.3, y1, 0.05)).Shape()
write(soic, out / "SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm.step")
write(box(-1.3, -1.1, 0.0, 1.3, 1.1, 0.55), out / "Broadcom_APDS-9301.step")
print("wrote", *sorted(p.name for p in out.glob("*.step")))
