#!/usr/bin/env python3
"""Score an exported schematic.

    python3 tools/schematic_quality.py board.kicad_sch [--json]

Needs kicad-cli. It plots the schematic to SVG and reads two things:

- wires from the `.kicad_sch` itself (every `(wire (pts ...))`, whatever its
  net class colour);
- text boxes from the plot: KiCad draws each string as a `stroked-text` group
  of glyph paths, so the box around the paths is where the text really is.

The numbers say how much of the page is collisions and how much is wasted:

  text_pairs_overlapping          pairs of texts whose boxes overlap
  texts_with_a_wire_through_them  texts with a wire running across them
  wire_crossings                  places two wires cross (a T is not one)
  wire_length_mm                  total wire
  region_coverage                 how much of the rectangle the group boxes span
                                  they cover (1.0 means they tile it, low means
                                  holes between and beside them)
  frame_coverage                  share of the page frame (inside the 20 mm margin,
                                  above the title block band) the group boxes
                                  cover (1.0 means the boxes fill the frame)
  max_gap_between_boxes_mm        widest gap between two group boxes that face
                                  each other (a box behind another is not a
                                  neighbour; 0.0 when none face)
  content_density                 share of 10 mm cells in the content box that
                                  hold a wire or text (low means wasted space;
                                  symbol outlines are not read, so it reads low)
  passive_to_pin_mean_mm          mean distance from a two-pin resistor or
  passive_to_pin_max_mm           capacitor (nearer pad) to the closest pin of
                                  another part on the same net, and the worst one
                                  (nets come from `kicad-cli sch export
                                  netlist`; a rail pin counts, so a part on a
                                  rail next to any pin of that rail reads near;
                                  0.0 when no passive could be measured)
  labels_repeating_pin_name       labels whose text is the name of the pin they
                                  stand beside
  items_outside_group_boxes       symbols and texts not wholly inside one group
                                  box (a symbol is the box round its pins and
                                  drawn outline; the page ruler and the title
                                  block are not content; 0 when there are no
                                  group boxes)
  title_gap_min_mm                smallest distance from a bold group caption
                                  down to the symbol, text or wire under it in
                                  its box (negative when they overlap; None when
                                  no caption has anything under it)
  flag_pairs_overlapping          pairs of net label flags whose boxes overlap
                                  (read from the `global_label` positions: a
                                  flag is FLAG_CHAR_MM per character plus
                                  FLAG_PAD_MM wide, FLAG_H_MM tall, and runs
                                  from its anchor away from its stub; boxes
                                  that overlap by less than FLAG_TOUCH_MM only
                                  touch, as flags on a 2.54 mm pin pitch do)
  flags_over_bodies               flags whose box overlaps the box of a placed
                                  symbol (its pin ends and drawn outline), by
                                  the same FLAG_TOUCH_MM; a flag runs out from
                                  its own pin's stub, so a part is only covered
                                  by the flag of another pin
  label_columns_misaligned        symbol sides whose labels do not line up: more
                                  than one distinct x on a left or right side,
                                  more than one distinct y on a top or bottom
                                  side (a label belongs to a pin when it sits on
                                  the pin's row or column, on the outward side,
                                  within LABEL_REACH_MM)

KiCad draws some strings twice in one place; each such string counts once.
Wires in Synth's schematics are axis aligned, so a wire is tested against a text
box by its bounding box.

They are a floor, not a grade: a schematic can score zero on all of them and
still be badly grouped. Lower is better for all but `content_density` and `region_coverage`.
"""

import json
import math
import re
import subprocess
import sys
import tempfile
from pathlib import Path

# Boxes and wires closer than this to touching do not count as overlapping.
TOLERANCE_MM = 0.05
# Two copies of one string at one place are KiCad drawing the text twice.
DUPLICATE_MM = 0.1
# A text this far outside the wires, symbols and group boxes is the page ruler
# or the title block, not schematic content.
CONTENT_MARGIN_MM = 5.0
# Cell size for the content density, in mm.
CELL_MM = 10.0
# A label this far or less from a pin's end, along the pin's axis and on its
# outward side, stands beside that pin: a stub is 5.08 mm.
LABEL_REACH_MM = 6.0
# A string this short, within this distance of the page edge, is a cell of the
# page ruler (1 to 8 across, A to F down), not schematic content.
RULER_CHARS = 2
RULER_BAND_MM = 12.0
# KiCad's stock title block: its texts plot up to 119 mm in from the right edge
# of the page and 40 mm up from the bottom, on A3 and on A2 alike.
TITLE_BLOCK_MM = (125.0, 45.0)
TITLE_BLOCK_INSET_MM = 2.0
# A caption's plotted text starts this close to its `(at)` in x and ends this
# close to it in y.
CAPTION_MATCH_MM = 2.5
# The box a global label output flag plots: 1.12 mm per character plus 2.9, and
# 2.65 tall. Flags that overlap by less than FLAG_TOUCH_MM only touch: flags on a
# 2.54 mm pin pitch are 0.11 mm taller than the pitch.
FLAG_CHAR_MM = 1.12
FLAG_PAD_MM = 2.9
FLAG_H_MM = 2.65
FLAG_TOUCH_MM = 0.2
# The page frame the group boxes should fill: in from every edge by the margin,
# and up from the bottom by the title block band (34 mm) and its text margin.
FRAME_MARGIN_MM = 20.0
FRAME_BOTTOM_MM = 40.35
# Longest kicad-cli may take to plot one schematic.
PLOT_TIMEOUT_S = 300

_NUMBER = r"[-\d.]+"


def _balanced(text, start):
    """The text of the bracket group that opens at `start`."""
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[start : i + 1]
    return text[start:]


def parse_wires(sch_text):
    """Every wire in a `.kicad_sch` as ((x1, y1), (x2, y2)) segments."""
    segments = []
    for opening in re.finditer(r"\(wire\b", sch_text):
        block = _balanced(sch_text, opening.start())
        points = [
            (float(x), float(y))
            for x, y in re.findall(rf"\(xy ({_NUMBER}) ({_NUMBER})\)", block)
        ]
        segments.extend(zip(points, points[1:]))
    return segments


def parse_content_points(sch_text):
    """Anchors of placed symbols and the corners of group boxes, as (x, y)."""
    points = [
        (float(x), float(y))
        for x, y in re.findall(
            rf'\(symbol\s+\(lib_id "[^"]*"\)\s+\(at ({_NUMBER}) ({_NUMBER})', sch_text
        )
    ]
    for x1, y1, x2, y2 in parse_group_boxes(sch_text):
        points += [(x1, y1), (x2, y2)]
    return points


def parse_group_boxes(sch_text):
    """The group boxes of a `.kicad_sch` as (x1, y1, x2, y2).

    They are the rectangles directly under the sheet; the rectangles inside a
    symbol definition are one level deeper and are not boxes."""
    boxes = []
    for opening in re.finditer(r"\n\t\(rectangle\b", sch_text):
        block = _balanced(sch_text, opening.start() + 2)
        corners = [
            (float(x), float(y))
            for x, y in re.findall(rf"\((?:start|end) ({_NUMBER}) ({_NUMBER})\)", block)
        ]
        if len(corners) == 2:
            (x1, y1), (x2, y2) = corners
            boxes.append((min(x1, x2), min(y1, y2), max(x1, x2), max(y1, y2)))
    return boxes


def parse_titles(sch_text):
    """The bold group captions of a `.kicad_sch` as (text, x, y); `(at)` is the
    left end of the baseline."""
    titles = []
    for opening in re.finditer(r"\n\t\(text\b", sch_text):
        block = _balanced(sch_text, opening.start() + 2)
        text = re.search(r'\(text\s+"([^"]*)"', block)
        at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER})", block)
        if text and at and re.search(r"\bbold\b", block):
            titles.append((text.group(1), float(at.group(1)), float(at.group(2))))
    return titles


def _lib_extents(sch_text):
    """The points that outline each library symbol, as {lib_id: [(x, y)]} in the
    symbol's own frame (y up): its pin ends and the corners of its rectangles
    and polylines."""
    points = {}
    start = sch_text.find("(lib_symbols")
    if start < 0:
        return points
    lib = _balanced(sch_text, start)
    for opening in re.finditer(r'\n\t\t\(symbol\s+"([^"]+)"', lib):
        block = _balanced(lib, opening.start() + 3)
        found = []
        for pin in re.finditer(r"\(pin\b", block):
            at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER}) {_NUMBER}\)", _balanced(block, pin.start()))
            if at:
                found.append((float(at.group(1)), float(at.group(2))))
        for shape in re.finditer(r"\((?:rectangle|polyline)\b", block):
            body = _balanced(block, shape.start())
            found += [
                (float(x), float(y))
                for x, y in re.findall(rf"\((?:start|end|xy) ({_NUMBER}) ({_NUMBER})\)", body)
            ]
        points[opening.group(1)] = found
    return points


def parse_symbol_boxes(sch_text):
    """Where each placed symbol is on the sheet, as (x1, y1, x2, y2): the box
    round its pin ends and its drawn outline, turned the way `_placed_pins`
    turns pins (the symbol's origin when it has neither)."""
    extents = _lib_extents(sch_text)
    boxes = []
    for opening in re.finditer(r"\n\t\(symbol\s+\(lib_id", sch_text):
        block = _balanced(sch_text, opening.start() + 2)
        lib_id = re.search(r'\(lib_id "([^"]*)"', block)
        at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER}) ({_NUMBER})", block)
        if not (lib_id and at):
            continue
        x0, y0, angle = (float(v) for v in at.groups())
        mirror = re.search(r"\(mirror (\w)\)", block)
        cos, sin = {0: (1, 0), 90: (0, 1), 180: (-1, 0), 270: (0, -1)}[round(angle) % 360]
        placed = []
        for lx, ly in extents.get(lib_id.group(1), []) or [(0.0, 0.0)]:
            if mirror and mirror.group(1) == "x":
                ly = -ly
            elif mirror and mirror.group(1) == "y":
                lx = -lx
            placed.append((x0 + lx * cos - ly * sin, y0 - (lx * sin + ly * cos)))
        xs, ys = zip(*placed)
        boxes.append((min(xs), min(ys), max(xs), max(ys)))
    return boxes


def _is_page_furniture(text, page):
    """The page ruler's cells and the text of the stock title block."""
    string, x0, y0, x1, y1 = text
    width, height = page
    edge = min(x0, y0, width - x1, height - y1)
    if len(string) <= RULER_CHARS and edge < RULER_BAND_MM:
        return True
    block_w, block_h = TITLE_BLOCK_MM
    return (
        x0 >= width - block_w
        and x1 <= width - TITLE_BLOCK_INSET_MM
        and y0 >= height - block_h
        and y1 <= height - TITLE_BLOCK_INSET_MM
    )


def items_outside_group_boxes(boxes, texts, symbol_boxes, page):
    """How many symbols and texts are not wholly inside one group box. With no
    group boxes there is nothing to be inside, so none count."""
    if not boxes:
        return 0
    items = [(t[1], t[2], t[3], t[4]) for t in texts if not _is_page_furniture(t, page)]
    items += symbol_boxes
    return sum(
        1
        for x0, y0, x1, y1 in items
        if not any(
            x0 >= bx0 - TOLERANCE_MM
            and y0 >= by0 - TOLERANCE_MM
            and x1 <= bx1 + TOLERANCE_MM
            and y1 <= by1 + TOLERANCE_MM
            for bx0, by0, bx1, by1 in boxes
        )
    )


def smallest_title_gap(boxes, titles, texts, symbol_boxes, wires):
    """The smallest distance in mm from a caption's text down to the symbol,
    text or wire under it, or None when no caption has anything under it.

    A caption is the plotted text with the string and position of a bold
    `(text)` of the schematic. What is under it lies in the group box that holds
    the caption, overlaps it in x, and does not end above the caption's top. The
    distance is from the caption's bottom to the item's top, negative when the
    item reaches into the caption."""
    plotted = [(t[0], t[1], t[2], t[3], t[4]) for t in texts]
    captions = [
        t
        for string, x, y in titles
        for t in plotted
        if t[0] == string and abs(t[1] - x) < CAPTION_MATCH_MM and abs(t[4] - y) < CAPTION_MATCH_MM
    ]
    items = [(t[1], t[2], t[3], t[4]) for t in plotted if t not in captions]
    items += symbol_boxes
    items += [(min(a[0], b[0]), min(a[1], b[1]), max(a[0], b[0]), max(a[1], b[1])) for a, b in wires]
    gaps = []
    for _, cx0, cy0, cx1, cy1 in captions:
        box = next(
            (b for b in boxes if b[0] <= cx0 and cx1 <= b[2] and b[1] <= cy0 and cy1 <= b[3]),
            None,
        )
        if box is None:
            continue
        gaps += [
            iy0 - cy1
            for ix0, iy0, ix1, iy1 in items
            if ix0 < cx1 and cx0 < ix1 and iy1 > cy0
            and ix0 < box[2] and box[0] < ix1 and iy0 < box[3] and box[1] < iy1
        ]
    return round(min(gaps), 2) if gaps else None


def region_coverage(boxes):
    """Sum of the box areas over the area of the rectangle that holds them all.

    Group boxes do not overlap, so the sum is the covered area."""
    if not boxes:
        return 0.0
    covered = sum((x2 - x1) * (y2 - y1) for x1, y1, x2, y2 in boxes)
    width = max(b[2] for b in boxes) - min(b[0] for b in boxes)
    height = max(b[3] for b in boxes) - min(b[1] for b in boxes)
    return round(covered / (width * height), 4) if width > 0 and height > 0 else 0.0


def frame_coverage(boxes, page):
    """Share of the page frame that the group boxes cover.

    The frame is the page inside the margin and above the title block band.
    Only the part of a box inside the frame counts; boxes do not overlap, so the
    sum is the covered area."""
    width, height = page
    fx0, fy0 = FRAME_MARGIN_MM, FRAME_MARGIN_MM
    fx1, fy1 = width - FRAME_MARGIN_MM, height - FRAME_BOTTOM_MM
    if fx1 <= fx0 or fy1 <= fy0:
        return 0.0
    covered = 0.0
    for x1, y1, x2, y2 in boxes:
        covered += max(0.0, min(x2, fx1) - max(x1, fx0)) * max(0.0, min(y2, fy1) - max(y1, fy0))
    return round(covered / ((fx1 - fx0) * (fy1 - fy0)), 4)


def max_gap_between_boxes_mm(boxes):
    """The widest gap between two group boxes that face each other.

    Two boxes face when they share rows (a gap across) or columns (a gap down)
    and no third box lies between them over the shared span, so a box farther
    off behind another is not a neighbour. 0.0 when no two boxes face."""
    widest = 0.0
    for i, a in enumerate(boxes):
        for b in boxes[i + 1 :]:
            for axis in (0, 1):
                other = 1 - axis
                first, second = (a, b) if a[axis] <= b[axis] else (b, a)
                gap = second[axis] - first[axis + 2]
                lo = max(first[other], second[other])
                hi = min(first[other + 2], second[other + 2])
                if gap < 0 or hi - lo <= TOLERANCE_MM:
                    continue
                between = any(
                    c is not first
                    and c is not second
                    and c[axis] >= first[axis + 2] - TOLERANCE_MM
                    and c[axis + 2] <= second[axis] + TOLERANCE_MM
                    and min(c[other + 2], hi) - max(c[other], lo) > TOLERANCE_MM
                    for c in boxes
                )
                if not between:
                    widest = max(widest, gap)
    return round(widest, 3)


def _lib_pins(sch_text):
    """Pins per library symbol as {lib_id: {pin number: (x, y, angle, name)}}, in
    the symbol's own frame (y up). A name that is empty or `~` is None."""
    pins = {}
    start = sch_text.find("(lib_symbols")
    if start < 0:
        return pins
    lib = _balanced(sch_text, start)
    for opening in re.finditer(r'\n\t\t\(symbol\s+"([^"]+)"', lib):
        block = _balanced(lib, opening.start() + 3)
        found = {}
        for pin in re.finditer(r"\(pin\b", block):
            body = _balanced(block, pin.start())
            at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER}) ({_NUMBER})", body)
            number = re.search(r'\(number\s+"([^"]*)"', body)
            name = re.search(r'\(name\s+"([^"]*)"', body)
            if at and number:
                x, y, angle = (float(v) for v in at.groups())
                label = name.group(1) if name and name.group(1) not in ("", "~") else None
                found[number.group(1)] = (x, y, angle, label)
        pins[opening.group(1)] = found
    return pins


def _placed_pins(sch_text):
    """Where each pin of each placed symbol is on the sheet, as
    {ref: (lib_id, {pin number: (x, y)})}. Mirror, turn counter-clockwise by
    the symbol's angle, then flip y: the sheet has y down. The mirror-then-turn
    order is only checked at angle 0; Synth's exporter writes no mirror."""
    lib = _lib_pins(sch_text)
    placed = {}
    for opening in re.finditer(r"\n\t\(symbol\s+\(lib_id", sch_text):
        block = _balanced(sch_text, opening.start() + 2)
        lib_id = re.search(r'\(lib_id "([^"]*)"', block)
        at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER}) ({_NUMBER})", block)
        ref = re.search(r'\(property\s+"Reference"\s+"([^"]*)"', block)
        if not (lib_id and at and ref and lib_id.group(1) in lib):
            continue
        x0, y0, angle = (float(v) for v in at.groups())
        mirror = re.search(r"\(mirror (\w)\)", block)
        cos, sin = {0: (1, 0), 90: (0, 1), 180: (-1, 0), 270: (0, -1)}[round(angle) % 360]
        pins = {}
        for number, (lx, ly, _, _) in lib[lib_id.group(1)].items():
            if mirror and mirror.group(1) == "x":
                ly = -ly
            elif mirror and mirror.group(1) == "y":
                lx = -lx
            pins[number] = (x0 + lx * cos - ly * sin, y0 - (lx * sin + ly * cos))
        placed[ref.group(1)] = (lib_id.group(1), pins)
    return placed


def _pin_facings(sch_text):
    """Per placed pin, which way it faces on the sheet and what it is called, as
    {ref: {pin number: ((dx, dy), name)}}, with (dx, dy) the unit step out of
    the symbol (y down). Same turn as `_placed_pins`."""
    facings = {}
    pins = _lib_pins(sch_text)
    for opening in re.finditer(r"\n\t\(symbol\s+\(lib_id", sch_text):
        block = _balanced(sch_text, opening.start() + 2)
        lib_id = re.search(r'\(lib_id "([^"]*)"', block)
        at = re.search(rf"\(at {_NUMBER} {_NUMBER} ({_NUMBER})", block)
        ref = re.search(r'\(property\s+"Reference"\s+"([^"]*)"', block)
        if not (lib_id and at and ref and lib_id.group(1) in pins):
            continue
        cos, sin = {0: (1, 0), 90: (0, 1), 180: (-1, 0), 270: (0, -1)}[round(float(at.group(1))) % 360]
        mirror = re.search(r"\(mirror (\w)\)", block)
        turned = {}
        for number, (_, _, pin_angle, name) in pins[lib_id.group(1)].items():
            # A pin at angle 0 sits left of its body, so it faces left.
            dx, dy = -math.cos(math.radians(pin_angle)), -math.sin(math.radians(pin_angle))
            if mirror and mirror.group(1) == "x":
                dy = -dy
            elif mirror and mirror.group(1) == "y":
                dx = -dx
            turned[number] = ((round(dx * cos - dy * sin), -round(dx * sin + dy * cos)), name)
        facings[ref.group(1)] = turned
    return facings


def label_pin_pairs(sch_text):
    """Every label that stands beside a pin, as (ref, facing, text, coordinate,
    pin name): facing is the pin's (dx, dy) and coordinate the label's x when the
    pin faces left or right, its y when it faces up or down. `label`,
    `global_label` and `hierarchical_label` all count."""
    placed = _placed_pins(sch_text)
    facings = _pin_facings(sch_text)
    pairs = []
    for opening in re.finditer(r'\((?:global_|hierarchical_)?label\s+"([^"]*)"', sch_text):
        block = _balanced(sch_text, opening.start())
        at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER})", block)
        if not at:
            continue
        lx, ly = float(at.group(1)), float(at.group(2))
        for ref, (_, pins) in placed.items():
            for number, (px, py) in pins.items():
                facing, name = facings[ref][number]
                dx, dy = facing
                along = (lx - px) * dx + (ly - py) * dy
                across = abs((lx - px) * dy) + abs((ly - py) * dx)
                if across < TOLERANCE_MM and 0 < along <= LABEL_REACH_MM:
                    pairs.append((ref, facing, opening.group(1), ly if dx == 0 else lx, name))
    return pairs


def _is_passive(lib_id):
    """A resistor or capacitor symbol from the Device library."""
    return re.match(r"Device:[RC](_|$)", lib_id) is not None


def passive_pin_distances(sch_text, netlist_text):
    """For each two-pin resistor or capacitor, the distance in mm from its
    nearer pad to the closest pin of another part on the same net (not a
    resistor, capacitor or power symbol; the symbol decides, not the
    reference, so `R_TNOW` is a resistor and `CONN1` is not a capacitor).

    A passive with no such pin on its nets is left out. `netlist_text` is the
    output of `kicad-cli sch export netlist`, which is where the nets come
    from: wires, labels and power symbols all end up in it."""
    placed = _placed_pins(sch_text)
    distances = {}
    for opening in re.finditer(r"\(net\b", netlist_text):
        block = _balanced(netlist_text, opening.start())
        nodes = re.findall(r'\(ref "([^"]+)"\)\s*\(pin "([^"]+)"\)', block)
        points = {
            (ref, pin): placed[ref][1][pin]
            for ref, pin in nodes
            if ref in placed and pin in placed[ref][1]
        }
        partners = [
            p for (ref, _), p in points.items()
            if not _is_passive(placed[ref][0]) and not ref.startswith("#")
        ]
        for (ref, _), pad in points.items():
            lib_id, pins = placed[ref]
            if len(pins) != 2 or not _is_passive(lib_id) or not partners:
                continue
            nearest = min(math.dist(pad, p) for p in partners)
            distances[ref] = min(nearest, distances.get(ref, nearest))
    return distances


def parse_texts(svg_text):
    """Every plotted string as (text, x0, y0, x1, y1)."""
    texts = []
    for group in re.finditer(
        r'<g class="stroked-text"><desc>(.*?)</desc>(.*?)</g>', svg_text, re.S
    ):
        points = [
            (float(x), float(y))
            for x, y in re.findall(rf"[ML]({_NUMBER}) ({_NUMBER})", group.group(2))
        ]
        if points:
            xs, ys = zip(*points)
            texts.append((group.group(1), min(xs), min(ys), max(xs), max(ys)))
    return _without_doubles(texts)


def _without_doubles(texts):
    """`texts` with a string KiCad drew twice in one place kept once."""
    kept = []
    for text in texts:
        if not any(_same_text_in_one_place(text, other) for other in kept):
            kept.append(text)
    return kept


def page_size(svg_text):
    """(width, height) of the plotted page in mm, from the SVG viewBox."""
    box = re.search(rf'viewBox="({_NUMBER}) ({_NUMBER}) ({_NUMBER}) ({_NUMBER})"', svg_text)
    return (float(box.group(3)), float(box.group(4))) if box else (0.0, 0.0)


def parse_flags(sch_text):
    """Every global label's flag as (text, x0, y0, x1, y1): the box from the label's
    anchor away from its stub, to the right at angle 0 and to the left at 180."""
    flags = []
    for opening in re.finditer(r'\(global_label\s+"([^"]*)"', sch_text):
        block = _balanced(sch_text, opening.start())
        at = re.search(rf"\(at ({_NUMBER}) ({_NUMBER}) ({_NUMBER})", block)
        if not at:
            continue
        x, y, angle = (float(v) for v in at.groups())
        text = opening.group(1)
        width = len(text) * FLAG_CHAR_MM + FLAG_PAD_MM
        x0, x1 = (x - width, x) if angle == 180 else (x, x + width)
        flags.append((text, x0, y - FLAG_H_MM / 2, x1, y + FLAG_H_MM / 2))
    return flags


def _boxes_overlap(a, b, tolerance=TOLERANCE_MM):
    return (
        a[1] < b[3] - tolerance
        and b[1] < a[3] - tolerance
        and a[2] < b[4] - tolerance
        and b[2] < a[4] - tolerance
    )


def _same_text_in_one_place(a, b):
    return a[0] == b[0] and all(abs(a[k] - b[k]) < DUPLICATE_MM for k in (1, 2, 3, 4))


def _segment_hits_box(segment, box):
    (x1, y1), (x2, y2) = segment
    return (
        min(x1, x2) < box[3] - TOLERANCE_MM
        and max(x1, x2) > box[1] + TOLERANCE_MM
        and min(y1, y2) < box[4] - TOLERANCE_MM
        and max(y1, y2) > box[2] + TOLERANCE_MM
    )


def _cross(p, q, r):
    return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])


def _segments_cross(s, t):
    """True when the segments cross inside both; touching ends do not."""
    (a, b), (c, d) = s, t
    return (
        _cross(a, b, c) * _cross(a, b, d) < 0 and _cross(c, d, a) * _cross(c, d, b) < 0
    )


def score(
    wires,
    texts,
    page,
    content_points=(),
    group_boxes=(),
    passive_distances=None,
    label_pairs=(),
    symbol_boxes=(),
    titles=(),
    flags=(),
):
    """The numbers in the module doc for `wires`, `texts` and `page` (w, h).

    `content_points` are anchors of symbols and corners of group boxes; with
    the wires they say where the schematic is, so page furniture is ignored.
    `label_pairs` are the labels beside pins, from `label_pin_pairs`;
    `symbol_boxes` and `titles` come from `parse_symbol_boxes` and `parse_titles`;
    `flags` come from `parse_flags`."""
    overlapping = 0
    for i, a in enumerate(texts):
        for b in texts[i + 1 :]:
            if _boxes_overlap(a, b):
                overlapping += 1

    flag_pairs = sum(
        1
        for i, a in enumerate(flags)
        for b in flags[i + 1 :]
        if _boxes_overlap(a, b, FLAG_TOUCH_MM)
    )

    flags_over_bodies = sum(
        1
        for f in flags
        for b in symbol_boxes
        if _boxes_overlap(f, ("", *b), FLAG_TOUCH_MM)
    )

    through = sum(
        1 for t in texts if any(_segment_hits_box(w, t) for w in wires)
    )

    crossings = sum(
        1
        for i, s in enumerate(wires)
        for t in wires[i + 1 :]
        if _segments_cross(s, t)
    )

    measured = list((passive_distances or {}).values())

    columns = {}
    for ref, facing, _, coordinate, _ in label_pairs:
        columns.setdefault((ref, facing), set()).add(round(coordinate / TOLERANCE_MM))

    length = sum(
        ((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5 for a, b in wires
    )

    return {
        "text_pairs_overlapping": overlapping,
        "texts_with_a_wire_through_them": through,
        "wire_crossings": crossings,
        "wire_length_mm": round(length, 1),
        "region_coverage": region_coverage(group_boxes),
        "frame_coverage": frame_coverage(group_boxes, page),
        "max_gap_between_boxes_mm": max_gap_between_boxes_mm(group_boxes),
        "content_density": _content_density(wires, texts, content_points),
        "passive_to_pin_mean_mm": round(sum(measured) / len(measured), 1) if measured else 0.0,
        "passive_to_pin_max_mm": round(max(measured), 1) if measured else 0.0,
        "items_outside_group_boxes": items_outside_group_boxes(
            group_boxes, texts, symbol_boxes, page
        ),
        "title_gap_min_mm": smallest_title_gap(group_boxes, titles, texts, symbol_boxes, wires),
        "labels_repeating_pin_name": sum(1 for p in label_pairs if p[2] == p[4]),
        "flag_pairs_overlapping": flag_pairs,
        "flags_over_bodies": flags_over_bodies,
        "label_columns_misaligned": sum(1 for c in columns.values() if len(c) > 1),
    }


def _content_density(wires, texts, content_points):
    """Share of the cells in the content box that hold a wire or a text."""
    xs = [p[0] for p in content_points]
    ys = [p[1] for p in content_points]
    for (x1, y1), (x2, y2) in wires:
        xs += [x1, x2]
        ys += [y1, y2]
    if not xs:
        return 0.0
    left, right = min(xs) - CONTENT_MARGIN_MM, max(xs) + CONTENT_MARGIN_MM
    top, bottom = min(ys) - CONTENT_MARGIN_MM, max(ys) + CONTENT_MARGIN_MM
    used = set()

    def mark(x0, y0, x1, y1):
        for i in range(int(x0 // CELL_MM), int(x1 // CELL_MM) + 1):
            for j in range(int(y0 // CELL_MM), int(y1 // CELL_MM) + 1):
                used.add((i, j))

    for (x1, y1), (x2, y2) in wires:
        steps = max(1, int(max(abs(x2 - x1), abs(y2 - y1)) // (CELL_MM / 2)) + 1)
        for k in range(steps + 1):
            x = x1 + (x2 - x1) * k / steps
            y = y1 + (y2 - y1) * k / steps
            mark(x, y, x, y)
    for x, y in content_points:
        mark(x, y, x, y)
    inside = []
    for _, x0, y0, x1, y1 in texts:
        if x0 >= left and x1 <= right and y0 >= top and y1 <= bottom:
            mark(x0, y0, x1, y1)
            inside.append((x0, y0, x1, y1))
    # The content box is the span of the wires, anchors and the texts kept.
    for x0, y0, x1, y1 in inside:
        xs += [x0, x1]
        ys += [y0, y1]
    cols = int(max(xs) // CELL_MM) - int(min(xs) // CELL_MM) + 1
    rows = int(max(ys) // CELL_MM) - int(min(ys) // CELL_MM) + 1
    return round(len(used) / (cols * rows), 4)


def pick_plot(plots, stem):
    """The SVG that plots the root sheet `stem`, from the plots kicad-cli wrote."""
    plots = sorted(plots)
    for plot in plots:
        if plot.stem == stem:
            return plot
    if len(plots) == 1:
        return plots[0]
    raise ValueError(
        f"kicad-cli wrote {len(plots)} plots and none is {stem}.svg: "
        f"{', '.join(p.name for p in plots) or 'no plots'}"
    )


def score_file(sch_path):
    """Plot `sch_path` with kicad-cli and score it."""
    sch_path = Path(sch_path).resolve()
    sch_text = sch_path.read_text()
    with tempfile.TemporaryDirectory() as tmp:
        try:
            subprocess.run(
                ["kicad-cli", "sch", "export", "svg", "-o", tmp, str(sch_path)],
                check=True,
                capture_output=True,
                text=True,
                timeout=PLOT_TIMEOUT_S,
            )
        except FileNotFoundError:
            raise SystemExit("kicad-cli not found: install KiCad 10 or put it on PATH")
        except subprocess.CalledProcessError as error:
            raise SystemExit(f"kicad-cli failed on {sch_path}:\n{error.stderr or error.stdout}")
        except subprocess.TimeoutExpired:
            raise SystemExit(f"kicad-cli took over {PLOT_TIMEOUT_S} s to plot {sch_path}")
        plot = pick_plot(list(Path(tmp).glob("*.svg")), sch_path.stem).read_text()
        net_path = Path(tmp) / "nets.net"
        try:
            subprocess.run(
                ["kicad-cli", "sch", "export", "netlist", "--format", "kicadsexpr",
                 "-o", str(net_path), str(sch_path)],
                check=True,
                capture_output=True,
                text=True,
                timeout=PLOT_TIMEOUT_S,
            )
        except subprocess.CalledProcessError as error:
            raise SystemExit(f"kicad-cli netlist failed on {sch_path}:\n{error.stderr or error.stdout}")
        except subprocess.TimeoutExpired:
            raise SystemExit(f"kicad-cli took over {PLOT_TIMEOUT_S} s to write the netlist of {sch_path}")
        netlist_text = net_path.read_text()
    return score(
        parse_wires(sch_text),
        parse_texts(plot),
        page_size(plot),
        parse_content_points(sch_text),
        parse_group_boxes(sch_text),
        passive_pin_distances(sch_text, netlist_text),
        label_pin_pairs(sch_text),
        parse_symbol_boxes(sch_text),
        parse_titles(sch_text),
        parse_flags(sch_text),
    )


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    if len(args) != 1:
        print(__doc__)
        return 2
    result = score_file(args[0])
    if "--json" in argv:
        print(json.dumps(result, indent=2))
    else:
        for name, value in result.items():
            print(f"{name:32} {value}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
