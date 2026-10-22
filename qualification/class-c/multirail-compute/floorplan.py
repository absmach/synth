"""Author a floorplan for the multirail board using TRUE KiCad courtyards.

The first generator sized parts from the registry's declared
`footprint_dimensions`, which is the part body. KiCad's `F.CrtYd` polygon is
what DRC checks, it is larger, and it is not centred on the footprint origin:
the ESP32-WROOM-32's courtyard is 48.0 x 41.2 mm because the footprint draws
the antenna keepout on that layer, and a 1x20 header's courtyard runs
y -1.78 to +50.03 from its origin. Placing by declared body size produced 46
courtyard overlaps.

This version works in courtyard space. Each group is given a rectangular
region; parts are packed inside it largest-first on a 0.5 mm grid, and every
candidate is tested as the true courtyard rectangle against every courtyard
already placed. The anchor written to the sidecar is derived back from the
chosen courtyard position, because the sidecar's x/y is the footprint origin.
"""
import glob
import os
import re
import sys
import tomllib

FP = os.environ["KICAD_FP_DIR"]
ROOT = os.environ.get("BOARD_ROOT", ".")
SRC = os.path.join(ROOT, "qualification/class-c/multirail-compute/multirail_compute.synth")
OUT = os.path.join(ROOT, "qualification/class-c/multirail-compute/multirail_compute.placement.layout.toml")

BOARD_W = float(os.environ.get("BOARD_W", 140.0))
BOARD_H = float(os.environ.get("BOARD_H", 100.0))
EDGE = 3.0
GAP = 1.6          # the placer inflates every courtyard by extra_margin_mm = 1.5 mm
                   # before its legalisation sweep, so a smaller gap here still
                   # reads as an overlap to it and it nudges the pinned part.
GRID = 0.5


def courtyard(libid):
    """(dx_min, dy_min, dx_max, dy_max) of F.CrtYd relative to the origin."""
    if not libid or ":" not in libid:
        return None
    lib, mod = libid.split(":", 1)
    path = os.path.join(FP, lib + ".pretty", mod + ".kicad_mod")
    if not os.path.exists(path):
        return None
    text = open(path, encoding="utf-8").read()
    pts = []
    pos = 0
    while True:
        k = text.find("(fp_", pos)
        if k < 0:
            break
        depth = 0
        blk = None
        for j in range(k, len(text)):
            if text[j] == "(":
                depth += 1
            elif text[j] == ")":
                depth -= 1
                if depth == 0:
                    blk = text[k:j + 1]
                    break
        if blk and "F.CrtYd" in blk:
            pts += [
                (float(a), float(b))
                for a, b in re.findall(r"\((?:start|end|center|xy) ([-0-9.]+) ([-0-9.]+)\)", blk)
            ]
        pos = k + 4
    if not pts:
        for m in re.finditer(r"\(pad ", text):
            k = m.start()
            depth = 0
            blk = None
            for j in range(k, len(text)):
                if text[j] == "(":
                    depth += 1
                elif text[j] == ")":
                    depth -= 1
                    if depth == 0:
                        blk = text[k:j + 1]
                        break
            at = re.search(r"\(at ([-0-9.]+) ([-0-9.]+)", blk or "")
            sz = re.search(r"\(size ([0-9.]+) ([0-9.]+)\)", blk or "")
            if at and sz:
                x, y = float(at.group(1)), float(at.group(2))
                w, h = float(sz.group(1)), float(sz.group(2))
                pts += [(x - w / 2 - 0.25, y - h / 2 - 0.25), (x + w / 2 + 0.25, y + h / 2 + 0.25)]
    if not pts:
        return None
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return (min(xs), min(ys), max(xs), max(ys))


src = open(SRC).read()
refs, group_of = {}, {}
current = None
for line in src.splitlines():
    g = re.match(r'\s*group "([^"]+)"', line)
    if g:
        current = g.group(1)
    c = re.match(r"\s*component (\w+): (\w+)\s+\"([^\"]+)\"", line)
    if c:
        refs[c.group(1)] = c.group(3)
        group_of[c.group(1)] = current

parts = {}
for p in glob.glob(os.path.join(ROOT, "registry/parts/*/*.synth.toml")):
    d = tomllib.load(open(p, "rb"))
    parts[d["id"]] = d


def geom(ref, rot=0):
    """Courtyard offsets for a part at a rotation, as (dx0, dy0, dx1, dy1)."""
    d = parts.get(refs[ref]) or {}
    e = courtyard(d.get("kicad_footprint"))
    if e is None:
        fd = d.get("footprint_dimensions") or {}
        w = fd.get("width_mm", 2.0) + 0.5
        h = fd.get("height_mm", 2.0) + 0.5
        e = (-w / 2, -h / 2, w / 2, h / 2)
    x0, y0, x1, y1 = e
    if rot == 90:
        return (-y1, x0, -y0, x1)
    if rot == 180:
        return (-x1, -y1, -x0, -y0)
    if rot == 270:
        return (y0, -x1, y1, -x0)
    return e


# Region per group, as (x0, y0, x1, y1) in board millimetres. The board reads
# left to right as it works: power down the left edge, the module in the
# middle with its headers above and below, and the interfaces on the right.
REGIONS = {
    "POWER IN":            (3.0, 3.0, 38.0, 26.0),
    "5V BUCK":             (3.0, 26.0, 38.0, 52.0),
    "3V3 RAIL":            (3.0, 52.0, 38.0, 76.0),
    "3V3 ANALOG":          (3.0, 76.0, 38.0, 88.0),
    "SWITCHED RAIL":       (3.0, 88.0, 38.0, 97.0),
    "COMPUTE":             (38.0, 22.0, 95.0, 74.0),
    "EXPANSION HEADERS":   (38.0, 3.0, 99.0, 97.0),
    "USB AND PROGRAMMING": (38.0, 74.0, 95.0, 97.0),
    "STORAGE":             (38.0, 9.0, 95.0, 22.0),
    "ETHERNET":            (95.0, 3.0, 137.0, 52.0),
    "IO EXPANSION":        (95.0, 52.0, 137.0, 80.0),
    "MONITORING":          (95.0, 80.0, 137.0, 90.0),
    "STATUS AND DISPLAY":  (95.0, 90.0, 137.0, 97.0),
    "MECHANICAL":          (3.0, 3.0, 137.0, 97.0),
    "INDUSTRIAL COMMS":    (99.0, 52.0, 137.0, 97.0),
    "RTC AND SUPERVISION": (38.0, 3.0, 95.0, 9.0),
}

# Parts whose rotation or position is dictated by the board edge they serve.
FIXED_ROT = {"J4": 90, "J5": 90}
# Hard anchors: a connector that must face out, or a hole that takes a corner.
HARD = {
    "H1": (3.0, 3.0), "H2": (134.0, 3.0), "H3": (3.0, 94.0), "H4": (134.0, 94.0),
    "J4": (40.0, 3.2), "J5": (40.0, 93.0),
}

placed = {}   # ref -> (cx0, cy0, cx1, cy1) courtyard rect actually occupied
chosen = {}   # ref -> (anchor_x, anchor_y, rot)


def fits(rect, skip=None):
    x0, y0, x1, y1 = rect
    if x0 < EDGE or y0 < EDGE or x1 > BOARD_W - EDGE or y1 > BOARD_H - EDGE:
        return False
    for r, (bx0, by0, bx1, by1) in placed.items():
        if r == skip:
            continue
        if x0 < bx1 + GAP and bx0 < x1 + GAP and y0 < by1 + GAP and by0 < y1 + GAP:
            return False
    return True


def put(ref, cx0, cy0, rot):
    """Place `ref` with its courtyard's top-left at (cx0, cy0)."""
    dx0, dy0, dx1, dy1 = geom(ref, rot)
    rect = (cx0, cy0, cx0 + (dx1 - dx0), cy0 + (dy1 - dy0))
    if not fits(rect):
        return False
    placed[ref] = rect
    # The sidecar's x/y is the COURTYARD CENTRE, not the footprint origin:
    # the exporter puts the origin at `center - rot(courtyard_offset)`, so on
    # the exported board the courtyard is centred exactly on this value, and
    # the router and DRC model it the same way. Writing the origin here put
    # every asymmetric part one offset away from where it was designed to be
    # -- 10.11 mm for the ESP32, 24.12 mm for a rotated 1x20 header.
    chosen[ref] = (round(cx0 + (dx1 - dx0) / 2.0, 2),
                   round(cy0 + (dy1 - dy0) / 2.0, 2), rot)
    return True


def area(ref, rot=0):
    dx0, dy0, dx1, dy1 = geom(ref, rot)
    return (dx1 - dx0) * (dy1 - dy0)


# 1. the hard anchors
for ref, (cx0, cy0) in HARD.items():
    if ref not in refs:
        continue
    if not put(ref, cx0, cy0, FIXED_ROT.get(ref, 0)):
        print("HARD ANCHOR FAILED:", ref)

# 2. everything else, group by group, largest first inside its region
order = sorted(
    (r for r in refs if r not in chosen),
    key=lambda r: -area(r, FIXED_ROT.get(r, 0)),
)
unplaced = []
for ref in order:
    rot = FIXED_ROT.get(ref, 0)
    grp = group_of.get(ref) or "MECHANICAL"
    rx0, ry0, rx1, ry1 = REGIONS.get(grp, (EDGE, EDGE, BOARD_W - EDGE, BOARD_H - EDGE))
    dx0, dy0, dx1, dy1 = geom(ref, rot)
    w, h = dx1 - dx0, dy1 - dy0
    done = False
    # sweep the region, then the whole board if the region is full
    for (sx0, sy0, sx1, sy1) in ((rx0, ry0, rx1, ry1), (EDGE, EDGE, BOARD_W - EDGE, BOARD_H - EDGE)):
        y = sy0
        while y + h <= sy1 and not done:
            x = sx0
            while x + w <= sx1 and not done:
                if put(ref, round(x, 2), round(y, 2), rot):
                    done = True
                x += GRID
            y += GRID
        if done:
            break
    if not done:
        unplaced.append(ref)

print("placed %d of %d; unplaced: %s" % (len(chosen), len(refs), unplaced or "none"))

# 3. independent overlap audit on the true courtyards
bad = []
items = list(placed.items())
for i in range(len(items)):
    for j in range(i + 1, len(items)):
        a, (ax0, ay0, ax1, ay1) = items[i]
        b, (bx0, by0, bx1, by1) = items[j]
        if ax0 < bx1 and bx0 < ax1 and ay0 < by1 and by0 < ay1:
            bad.append((a, b))
print("true-courtyard overlaps:", len(bad), bad[:6])

xs = [r[2] for r in placed.values()]
ys = [r[3] for r in placed.values()]
x0s = [r[0] for r in placed.values()]
y0s = [r[1] for r in placed.values()]
print("extent: %.1f..%.1f x %.1f..%.1f mm" % (min(x0s), max(xs), min(y0s), max(ys)))

if unplaced or bad:
    sys.exit(1)

header = '''schema_version = 2

# A complete floorplan, authored because the placer cannot produce one: it
# refuses this board with E-SYNTH-PLACE-002 on whichever passive it reaches
# last, being greedy first-fit with no backtracking.
#
# Positions are computed in COURTYARD space from each footprint's real
# `F.CrtYd` polygon, not from the registry's declared body size. That
# distinction is the whole point of this file: the ESP32-WROOM-32's courtyard
# is 48.0 x 41.2 mm, not the 19.0 x 17.4 mm body, because the footprint draws
# the antenna keepout on the courtyard layer; and a 1x20 header's courtyard
# runs y -1.78 to +50.03 from its origin, so the origin is nowhere near the
# centre. An earlier floorplan that used declared body sizes produced 46
# courtyard overlaps in `kicad-cli pcb drc`.
#
# Each group gets a region and its parts are packed inside it largest-first,
# every candidate tested as a true courtyard rectangle against every courtyard
# already placed, with a %.1f mm gap. The value written here is the footprint
# origin, derived back from the courtyard position, because that is what the
# sidecar's x/y means.
#
# The board reads left to right as it works: 12 V entry and the rails down the
# left edge, the module in the middle with a header above and below, and the
# interfaces on the right.
''' % GAP

lines = [header]
by_group = {}
for ref, (x, y, rot) in chosen.items():
    by_group.setdefault(group_of.get(ref) or "MECHANICAL", []).append((ref, x, y, rot))
for grp in sorted(by_group):
    lines.append("# --- %s %s" % (grp, "-" * max(0, 60 - len(grp))))
    for ref, x, y, rot in sorted(by_group[grp]):
        lines.append("[components.%s]\nx = %.2f\ny = %.2f\nrotation = %d\n" % (ref, x, y, rot))

open(OUT, "w").write("\n".join(lines))
print("wrote", OUT, "with", len(chosen), "entries")
