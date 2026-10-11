"""Pad-edge gaps (mm) for the buck loop pairs. Usage: python3 -I pad_gaps.py buck.kicad_pcb
Needs the pcbnew Python module that ships with KiCad; run with python3 -I."""
import math
import sys

import pcbnew

PAIRS = [("C2", "1", "U1", "3", "C2 to U1 VIN"), ("C3", "1", "U1", "3", "C3 to U1 VIN"),
         ("C2", "2", "U1", "1", "C2 GND pad to U1 GND"), ("C3", "2", "U1", "1", "C3 GND pad to U1 GND"),
         ("C4", "1", "U1", "6", "C4 to U1 BOOT"), ("C4", "2", "U1", "2", "C4 to U1 SW"),
         ("L1", "1", "U1", "2", "L1 pad 1 to U1 SW"), ("C5", "1", "L1", "2", "C5 to L1 pad 2"),
         ("C6", "1", "L1", "2", "C6 to L1 pad 2"), ("D2", "1", "U1", "3", "D2 TVS to U1 VIN"),
         ("R_FB_T", "2", "U1", "4", "R_FB_T to U1 FB"), ("R_FB_B", "1", "U1", "4", "R_FB_B to U1 FB"),
         ("C7", "1", "R_FB_T", "2", "C7 to R_FB_T (FB side)"), ("R_FB_B", "2", "U1", "1", "R_FB_B GND to U1 GND"),
         ("R_FB_T", "2", "L1", "1", "R_FB_T FB pad to L1 pad 1 (SW)")]
board = pcbnew.LoadBoard(sys.argv[1])
def box(ref, num):
    return board.FindFootprintByReference(ref).FindPadByNumber(num).GetBoundingBox()
for ra, na, rb, nb, name in PAIRS:
    a, b = box(ra, na), box(rb, nb)
    dx = max(0, max(a.GetLeft(), b.GetLeft()) - min(a.GetRight(), b.GetRight()))
    dy = max(0, max(a.GetTop(), b.GetTop()) - min(a.GetBottom(), b.GetBottom()))
    print(f"{math.hypot(dx, dy) / 1e6:6.2f} mm  {name}")
