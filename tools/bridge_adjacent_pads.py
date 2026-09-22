#!/usr/bin/env python3
import sys
import pcbnew

board = pcbnew.LoadBoard(sys.argv[1])
plane_codes = {zone.GetNetCode() for zone in board.Zones()}
added = 0
for footprint in board.GetFootprints():
    pads = [
        pad for pad in footprint.Pads()
        if pad.GetAttribute() == 0 and pad.GetNetCode() > 0
        and pad.GetNetCode() not in plane_codes
    ]
    for index, first in enumerate(pads):
        for second in pads[index + 1:]:
            if first.GetNetCode() != second.GetNetCode():
                continue
            a, b = first.GetPosition(), second.GetPosition()
            dx, dy = abs(b.x - a.x), abs(b.y - a.y)
            pitch = max(dx, dy)
            if not (pcbnew.FromMM(2.45) <= pitch <= pcbnew.FromMM(2.65)):
                continue
            if min(dx, dy) > pcbnew.FromMM(0.01):
                continue
            track = pcbnew.PCB_TRACK(board)
            track.SetStart(a)
            track.SetEnd(b)
            track.SetWidth(pcbnew.FromMM(0.127))
            track.SetLayer(pcbnew.B_Cu)
            track.SetNetCode(first.GetNetCode())
            board.Add(track)
            added += 1
if added:
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
pcbnew.SaveBoard(sys.argv[2], board)
print(f"added {added} adjacent-pad bridges")
