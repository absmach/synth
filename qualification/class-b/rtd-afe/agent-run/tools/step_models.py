#!/usr/bin/env python3
"""step_models.py <out dir>: write the 3D models the RTD AFE needs and KiCad's stock 3D library lacks.

Run with a Python that has OCP (OpenCASCADE), for example `pip install cadquery-ocp` in a venv. Each file is named after
its KiCad footprint, in millimetres, origin at the footprint origin, +z up from the board surface, pin 1 at the top left
(KiCad's model frame is y-up, so the footprint's y = -model y). These are simplified bodies built from datasheet
dimensions, not vendor CAD: a body, leads and exposed pad (or shell and opening), with a pin-1 dimple on the ICs.

  Texas_RHB0032M_VQFN-32-1EP_5x5mm_P0.5mm_EP2.1x2.1mm.step   ADS124S08, TI datasheet Rev. C, RHB0032M package:
      https://www.ti.com/lit/ds/symlink/ads124s08.pdf  5.0 x 5.0 mm body, height 1.0 mm max (0.8 nominal, 0.9 used),
      standoff 0.00 to 0.05 mm (0.02 used), lead length 0.3 to 0.5 mm (0.4 used), lead width 0.2 to 0.3 mm (0.25 used),
      pitch 0.5 mm, exposed pad 2.1 mm square (the footprint's pad 33).
  QFN-60-1EP_7x7mm_P0.4mm_EP3.4x3.4mm.step                   RP2350A, Raspberry Pi RP2350 datasheet, package section:
      https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf  7.0 x 7.0 mm body, height 0.80 / 0.85 / 0.90 mm
      (0.85 used), standoff 0 to 0.05 mm (0.02 used), lead length 0.35 to 0.45 mm (0.4 used), lead width 0.20 to 0.30 mm
      (0.25 used), pitch 0.4 mm. The exposed pad is the footprint's 3.4 mm square (pad 61), not the datasheet table's value.
  USB_C_Receptacle_HRO_TYPE-C-31-M-12.step                   HRO TYPE-C-31-M-12 (LCSC C165948), vendor drawing
      http://www.krhro.com/uploads/soft/180320/1-1P320120243.pdf (403 to our fetch, not read): shell 8.94 mm wide (the
      footprint's Fab outline, x +-4.47) and 7.30 mm deep (Fab outline, y +-3.65), 3.21 mm high (the bounding box of a
      community model of this part, ai03-2725/Type-C.pretty, used as a cross-check only); mating opening 8.34 x 2.56 mm (USB
      Type-C R2.0 receptacle interface), 1.6 mm above the board, cut 6.5 mm into the front (footprint +y) face.
      No shell legs or contact tails are modelled.

A lead is 0.2 mm thick, an assumption (no datasheet value).
"""
import sys
from pathlib import Path

from OCP.BRep import BRep_Builder
from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Writer
from OCP.TopoDS import TopoDS_Compound


def box(x0, y0, z0, x1, y1, z1):
    return BRepPrimAPI_MakeBox(gp_Pnt(x0, y0, z0), gp_Pnt(x1, y1, z1)).Shape()


def compound(shapes):
    builder, result = BRep_Builder(), TopoDS_Compound()
    builder.MakeCompound(result)
    for shape in shapes:
        builder.Add(result, shape)
    return result


def write(shape, path):
    writer = STEPControl_Writer()
    writer.Transfer(shape, STEPControl_AsIs)
    if writer.Write(str(path)) != IFSelect_RetDone:
        raise SystemExit(f"could not write {path}")


def qfn(per_side, pitch, size, height, lead_len, lead_w, ep, standoff=0.02, lead_t=0.2):
    h, span = size / 2, (per_side - 1) * pitch / 2
    body = box(-h, -h, standoff, h, h, height)
    marker = BRepPrimAPI_MakeCylinder(gp_Ax2(gp_Pnt(-h + 0.6, h - 0.6, height - 0.05), gp_Dir(0, 0, 1)), 0.15, 0.1).Shape()
    parts = [BRepAlgoAPI_Cut(body, marker).Shape(), box(-ep / 2, -ep / 2, 0, ep / 2, ep / 2, standoff)]
    for i in range(per_side):
        c = -span + i * pitch
        parts += [
            box(-h, c - lead_w / 2, 0, -h + lead_len, c + lead_w / 2, lead_t),
            box(h - lead_len, c - lead_w / 2, 0, h, c + lead_w / 2, lead_t),
            box(c - lead_w / 2, -h, 0, c + lead_w / 2, -h + lead_len, lead_t),
            box(c - lead_w / 2, h - lead_len, 0, c + lead_w / 2, h, lead_t),
        ]
    return compound(parts)


def usb_c_receptacle():
    shell = box(-4.47, -3.65, 0, 4.47, 3.65, 3.21)
    opening = box(-4.17, -3.65, 1.6 - 1.28, 4.17, -3.65 + 6.5, 1.6 + 1.28)
    return BRepAlgoAPI_Cut(shell, opening).Shape()


out = Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
write(qfn(8, 0.5, 5.0, 0.9, 0.4, 0.25, 2.1), out / "Texas_RHB0032M_VQFN-32-1EP_5x5mm_P0.5mm_EP2.1x2.1mm.step")
write(qfn(15, 0.4, 7.0, 0.85, 0.4, 0.25, 3.4), out / "QFN-60-1EP_7x7mm_P0.4mm_EP3.4x3.4mm.step")
write(usb_c_receptacle(), out / "USB_C_Receptacle_HRO_TYPE-C-31-M-12.step")
print("wrote", *sorted(p.name for p in out.glob("*.step")))
