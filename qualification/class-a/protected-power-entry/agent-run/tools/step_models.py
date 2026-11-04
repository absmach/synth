#!/usr/bin/env python3
"""step_models.py <out dir>: write the 3D models KiCad's stock library does not have, as STEP files.

Run with a Python that has OCP (OpenCASCADE), for example `pip install cadquery-ocp` in a venv.
Each model is in millimetres with its origin at the footprint origin and +z up from the board surface.
These are envelope models of the real parts, not manufacturer CAD:

  TestPoint_Pad_D1.5mm.step    a 1.5 mm disc of copper, 0.05 mm proud of the board surface.
  Fuse_1812_4532Metric.step    4.5 x 3.2 mm body, 1.25 mm high (JLCPCB listing of the Littelfuse 1812L150/16DR,
                               C207070), with 0.6 mm end terminations at the pads.


A mounting hole has no body: the hole is drilled in the board solid of the STEP.
"""
import sys
from pathlib import Path

from OCP.BRepAlgoAPI import BRepAlgoAPI_Fuse
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer


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
out.mkdir(parents=True, exist_ok=True)
write(disc(0.75, 0.05), out / "TestPoint_Pad_D1.5mm.step")
write(
    fuse_all(
        [
            box(-1.65, -1.6, 0.0, 1.65, 1.6, 1.25),
            box(-2.25, -1.6, 0.0, -1.65, 1.6, 1.25),
            box(1.65, -1.6, 0.0, 2.25, 1.6, 1.25),
        ]
    ),
    out / "Fuse_1812_4532Metric.step",
)
print("wrote", *sorted(p.name for p in out.glob("*.step")))
