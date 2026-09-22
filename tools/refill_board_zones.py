#!/usr/bin/env python3
import sys
import pcbnew

board = pcbnew.LoadBoard(sys.argv[1])
if board.Zones():
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
pcbnew.SaveBoard(sys.argv[2], board)
