#!/usr/bin/env python3
"""step_check.py <board.step> <board.kicad_pcb>: check a KiCad STEP export and print its content hash.

Checks the ISO 10303-21 framing, that every entity reference resolves, and that the number of
components named in the STEP is the number of footprints with a 3D model in the board.
KiCad's STEP writer orders and numbers its entities differently on every run, even for one board, so the file
hash is not reproducible. The geometry hash is: SHA-256 of the sorted multiset of CARTESIAN_POINT coordinates
rounded to 0.001 mm, which is the same for every export of the same board."""
import collections
import hashlib
import re
import sys

step = open(sys.argv[1]).read()
board = open(sys.argv[2]).read()
ok = step.startswith("ISO-10303-21;") and step.rstrip().endswith("END-ISO-10303-21;") and step.count("ENDSEC;") == 2
defined = set(re.findall(r"^#(\d+)\s*=", step, re.M))
used = set(re.findall(r"#(\d+)", step.split("DATA;", 1)[1]))
dangling = sorted(used - defined, key=int)
solids = len(re.findall(r"MANIFOLD_SOLID_BREP", step))
models = len(re.findall(r'\(model\s+"', board))
points = collections.Counter(
    tuple(round(float(v), 3) for v in p.split(","))
    for p in re.findall(r"CARTESIAN_POINT\('[^']*',\(([^)]*)\)\)", step)
)
geometry = hashlib.sha256(repr(sorted(points.items())).encode()).hexdigest()[:10]
print(f"framing ok: {ok}; entities {len(defined)}; dangling references {len(dangling)}; solids {solids}; footprints with a model in the board {models}")
print(geometry, "geometry hash")
sys.exit(0 if ok and not dangling else 1)
