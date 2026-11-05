#!/usr/bin/env python3
"""delivered_counts.py <export dir> <name>: count what the delivered board holds and set it beside the routing report.

The routing report's segment, via and wire-length statistics are taken before Synth's repair passes (the
dangling-stub prune, the island ties and the return vias); the delivered board is the repaired one. This reads the
delivered .kicad_pcb and prints both, as JSON."""
import collections
import hashlib
import json
import math
import re
import sys
from pathlib import Path

export, name = Path(sys.argv[1]), sys.argv[2]
pcb_path = export / f"{name}.kicad_pcb"
pcb = pcb_path.read_text()
report = json.loads((export / f"{name}.routing.json").read_text())
nets = {int(n): s for n, s in re.findall(r'\(net (\d+) "([^"]*)"\)', pcb)}
segments = re.findall(
    r"\(segment\s+\(start ([\d.-]+) ([\d.-]+)\)\s*\(end ([\d.-]+) ([\d.-]+)\)\s*\(width ([\d.]+)\)", pcb
)
vias = [
    (re.search(r"\(size ([\d.]+)\)", b).group(1), re.search(r"\(drill ([\d.]+)\)", b).group(1))
    for b in re.findall(r"\(via\b(.*?)\n\s*\)", pcb, re.S)
]
length = sum(math.hypot(float(a) - float(c), float(b) - float(d)) for a, b, c, d, _ in segments)
stats = report["statistics"]
out = {
    "board": f"{name}.kicad_pcb",
    "board_sha256_10": hashlib.sha256(pcb_path.read_bytes()).hexdigest()[:10],
    "delivered_board": {
        "segments": len(segments),
        "vias": len(vias),
        "via_size_drill_mm": {f"{s}/{d}": n for (s, d), n in sorted(collections.Counter(vias).items())},
        "wire_length_mm": round(length, 1),
    },
    "routing_report": {
        "segments": stats["segments"],
        "vias": stats["vias"],
        "wire_length_mm": round(stats["wire_length_mm"], 1),
        "connected_nets": stats["connected_nets"],
        "open_nets": stats["open_nets"],
        "note": "connected_nets and open_nets are refreshed from the delivered board's validation; the others are not",
    },
    "kicad_drc": report["validation"]["kicad_drc"],
}
print(json.dumps(out, indent=2))
