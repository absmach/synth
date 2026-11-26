#!/usr/bin/env python3
"""step_models.py <out dir>: write the 3D model the buck board needs and KiCad's stock 3D library lacks.

Run with a Python that has OCP (OpenCASCADE), for example `pip install cadquery-ocp` in a venv. Millimetres, origin at
the footprint origin, +z up from the board surface. Same construction as the protected power entry's
agent-run/tools/step_models.py (three solids: body and two terminations), with this part's dimensions.

  Fuse_1812_4532Metric.step  F1, Bourns MF-MSMF110/33X, https://www.bourns.com/docs/Product-Datasheets/mfmsmf.pdf, dimensions
                             table, row MF-MSMF110/33X, at the middle of the datasheet ranges: 4.60 mm long (A 4.37 to 4.83),
                             3.24 mm wide (B 3.07 to 3.41), 1.50 mm thick (C 1.20 to 1.80), end terminations 0.75 mm long
                             (D 0.30 to 1.20). A simplified body, not the manufacturer's CAD.
"""
import sys
from pathlib import Path

from OCP.BRep import BRep_Builder
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox
from OCP.gp import gp_Pnt
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer
from OCP.TopoDS import TopoDS_Compound


def box(x0, y0, z0, x1, y1, z1):
    return BRepPrimAPI_MakeBox(gp_Pnt(x0, y0, z0), gp_Pnt(x1, y1, z1)).Shape()


out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
builder, fuse = BRep_Builder(), TopoDS_Compound()
builder.MakeCompound(fuse)
for part in (
    box(-1.55, -1.62, 0.0, 1.55, 1.62, 1.50),
    box(-2.30, -1.62, 0.0, -1.55, 1.62, 1.50),
    box(1.55, -1.62, 0.0, 2.30, 1.62, 1.50),
):
    builder.Add(fuse, part)
writer = STEPControl_Writer()
writer.Transfer(fuse, STEPControl_AsIs)
if writer.Write(str(out / "Fuse_1812_4532Metric.step")) != IFSelect_RetDone:
    raise SystemExit("could not write the fuse model")
