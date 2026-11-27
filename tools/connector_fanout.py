#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Connector-dogbone escape for a two-row fine-pitch connector (absmach/synth#433).

A 0.4 mm-pitch board-to-board connector (the CM4's DF40: 0.2 x 0.7 mm pads,
two rows 3.08 mm apart) leaves no room for a standard via at a pad or between
two pads of a row. It leaves a channel between the rows. This helper gives
every netted SMD pad of the named footprint a short stub into that channel and
a microvia at its end, on a staggered grid: alternate pads of a row put their
via one via pitch further in, so neighbouring vias and the stubs that pass
them keep their clearance. Every via is a laser microvia: it spans one layer
pair, from the pad's outer layer to the copper layer next to it (In1.Cu under
F.Cu, the last inner layer over B.Cu), which must be among the declared
layers. A via that skipped a layer would be a blind via, a different and
dearer process step, so the helper refuses rather than writing one.

Every clearance the issue asks for is checked on the geometry written, not
assumed from the grid: no via drill touches an SMD pad, and via-to-pad,
via-to-track and via-to-via clearance between different nets is at least
`--clearance`. The vias and stubs are also checked against the rest of the
board: other parts' pads on the layers they occupy, every drilled or
non-plated hole, existing tracks and vias, and the Edge.Cuts outline (at
`--edge-clearance`). A pad that cannot be escaped fails the helper with its
name; nothing is dropped silently. The board is written only when every pad
escaped.

The last line printed is the `JSON_SUMMARY:` ledger the routing helper reads,
in bga_fanout's shape: `requested`, `escaped`, `failed`, `unescaped_nets`.
"""

from __future__ import annotations

import argparse
import itertools
import json
import math
import re
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path

ESCAPE_FAILED = 4


@dataclass
class Pad:
    name: str
    net_number: int | None
    net_name: str
    # Footprint-local centre, as stored in the file.
    x: float
    y: float
    width: float
    height: float
    # Pad angle relative to the footprint.
    angle: float
    layer: str


@dataclass
class Footprint:
    reference: str
    x: float
    y: float
    angle: float
    pads: list[Pad]


def _balanced(text: str, start: int) -> int:
    """Index one past the parenthesis that closes the one at `start`."""
    depth = 0
    in_string = False
    i = start
    while i < len(text):
        c = text[i]
        if in_string:
            if c == "\\":
                i += 1
            elif c == '"':
                in_string = False
        elif c == '"':
            in_string = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced s-expression")


def _children(block: str, head: str) -> list[str]:
    """Top-level child lists of `block` whose head is `head`."""
    out = []
    i = 1
    depth_one = block.index(" ", 1) if " " in block else len(block)
    i = depth_one
    while i < len(block):
        if block[i] == "(":
            end = _balanced(block, i)
            child = block[i:end]
            if re.match(rf"\({re.escape(head)}[\s)]", child):
                out.append(child)
            i = end
        elif block[i] == '"':
            i = block.index('"', i + 1) + 1
        else:
            i += 1
    return out


def _numbers(block: str, head: str) -> list[float]:
    found = _children(block, head)
    if not found:
        return []
    words = found[0][len(head) + 1:-1].split()
    out = []
    for word in words:
        try:
            out.append(float(word))
        except ValueError:
            break
    return out


def footprints(text: str) -> list[tuple[int, int, str]]:
    """(start, end, block) of every top-level footprint."""
    out = []
    for match in re.finditer(r"\(footprint\s", text):
        start = match.start()
        end = _balanced(text, start)
        out.append((start, end, text[start:end]))
    return out


def parse_footprint(block: str) -> Footprint | None:
    reference = re.search(r'\(property\s+"Reference"\s+"([^"]*)"', block)
    if not reference:
        reference = re.search(r'\(fp_text\s+reference\s+"?([^"\s)]*)', block)
    if not reference:
        return None
    at = _numbers(block, "at")
    if len(at) < 2:
        return None
    pads = []
    for pad in _children(block, "pad"):
        head = re.match(r'\(pad\s+"([^"]*)"\s+(\w+)', pad)
        if not head or head.group(2) != "smd":
            continue
        net = re.search(r'\(net\s+(\d+)\s+"([^"]*)"\)', pad) or re.search(
            r'\(net\s+"([^"]*)"\)', pad
        )
        if not net:
            continue
        if net.re.groups == 2:
            number, name = int(net.group(1)), net.group(2)
        else:
            number, name = None, net.group(1)
        pad_at = _numbers(pad, "at")
        size = _numbers(pad, "size")
        layers = re.search(r'\(layers\s+"([FB])\.Cu"', pad)
        if len(pad_at) < 2 or len(size) < 2 or not layers:
            continue
        absolute = pad_at[2] if len(pad_at) > 2 else 0.0
        pads.append(Pad(
            name=head.group(1), net_number=number, net_name=name,
            x=pad_at[0], y=pad_at[1], width=size[0], height=size[1],
            angle=(absolute - (at[2] if len(at) > 2 else 0.0)) % 360.0,
            layer=f"{layers.group(1)}.Cu",
        ))
    return Footprint(reference.group(1), at[0], at[1], at[2] if len(at) > 2 else 0.0, pads)


def to_board(fp: Footprint, x: float, y: float) -> tuple[float, float]:
    """Footprint-local to board coordinates (KiCad: y down, angle CCW)."""
    a = math.radians(fp.angle)
    return (
        fp.x + x * math.cos(a) + y * math.sin(a),
        fp.y - x * math.sin(a) + y * math.cos(a),
    )


# --- geometry for the clearance check, all in footprint-local mm -------------

def _point_segment(px, py, ax, ay, bx, by) -> float:
    dx, dy = bx - ax, by - ay
    length2 = dx * dx + dy * dy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length2))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def _segments_cross(a, b, c, d) -> bool:
    def orient(p, q, r):
        return (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    o1, o2, o3, o4 = orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b)
    return (o1 * o2 < 0) and (o3 * o4 < 0)


def _segment_segment(a, b, c, d) -> float:
    if _segments_cross(a, b, c, d):
        return 0.0
    return min(
        _point_segment(*a, *c, *d), _point_segment(*b, *c, *d),
        _point_segment(*c, *a, *b), _point_segment(*d, *a, *b),
    )


def _rect(pad: Pad) -> tuple[float, float, float, float]:
    """Axis-aligned extent; the escape only handles 0/90/180/270 pads."""
    turned = round(pad.angle / 90.0) % 2 == 1
    w, h = (pad.height, pad.width) if turned else (pad.width, pad.height)
    return (pad.x - w / 2, pad.y - h / 2, pad.x + w / 2, pad.y + h / 2)


def _segment_rect(a, b, rect) -> float:
    x0, y0, x1, y1 = rect
    inside = lambda p: x0 <= p[0] <= x1 and y0 <= p[1] <= y1  # noqa: E731
    if inside(a) or inside(b):
        return 0.0
    corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    return min(_segment_segment(a, b, corners[i], corners[(i + 1) % 4]) for i in range(4))


@dataclass
class Escape:
    pad: Pad
    via: tuple[float, float]
    target: str


def plan(fp: Footprint, target: str, via_size: float, via_drill: float,
         stub_width: float, clearance: float) -> tuple[list[Escape], list[str]]:
    """Place each pad's via; return the escapes and the pads that cannot be."""
    pads = fp.pads
    if len(pads) < 4:
        return [], [p.name for p in pads] or ["(no netted SMD pads)"]
    skewed = [p.name for p in pads if abs(p.angle - round(p.angle / 90.0) * 90.0) > 0.01]
    if skewed:
        return [], [f"{name}: pad is not at a right angle to the footprint" for name in skewed]
    xs = [p.x for p in pads]
    ys = [p.y for p in pads]
    # Rows run along the longer spread; the row coordinate is the other axis.
    along_x = (max(xs) - min(xs)) >= (max(ys) - min(ys))
    row_of = (lambda p: p.y) if along_x else (lambda p: p.x)
    rows: dict[float, list[Pad]] = {}
    for pad in pads:
        key = next((k for k in rows if abs(k - row_of(pad)) < 0.05), round(row_of(pad), 3))
        rows.setdefault(key, []).append(pad)
    if len(rows) != 2:
        return [], [f"{fp.reference} has {len(rows)} pad rows, not 2"]
    (low, low_pads), (high, high_pads) = sorted(rows.items())
    radius = via_size / 2
    step = via_size + clearance
    escapes: list[Escape] = []
    for row_pads, inward, row in ((low_pads, 1.0, low), (high_pads, -1.0, high)):
        for index, pad in enumerate(sorted(row_pads, key=lambda p: p.x if along_x else p.y)):
            x0, y0, x1, y1 = _rect(pad)
            edge = (y1 if inward > 0 else y0) if along_x else (x1 if inward > 0 else x0)
            line = index % 2
            depth = edge + inward * (clearance + radius + line * step)
            via = (pad.x, depth) if along_x else (depth, pad.y)
            escapes.append(Escape(pad, via, target))
    return escapes, check(fp, escapes, via_size, via_drill, stub_width, clearance)


def check(fp: Footprint, escapes: list[Escape], via_size: float, via_drill: float,
          stub_width: float, clearance: float) -> list[str]:
    """Pads whose escape breaks a clearance, each with the reason."""
    radius, drill_r, half = via_size / 2, via_drill / 2, stub_width / 2
    bad: list[str] = []
    for e in escapes:
        stub = ((e.pad.x, e.pad.y), e.via)
        for pad in fp.pads:
            rect = _rect(pad)
            gap = _segment_rect(e.via, e.via, rect)
            if gap < drill_r:
                bad.append(f"{e.pad.name}: via drill touches pad {pad.name}")
            elif pad.net_name != e.pad.net_name and gap - radius < clearance - 1e-9:
                bad.append(f"{e.pad.name}: via {gap - radius:.3f} mm from pad {pad.name}")
            if pad.net_name != e.pad.net_name and _segment_rect(*stub, rect) - half < clearance - 1e-9:
                bad.append(f"{e.pad.name}: stub too close to pad {pad.name}")
        for other in escapes:
            if other is e or other.pad.net_name == e.pad.net_name:
                continue
            centres = math.dist(e.via, other.via)
            if centres - 2 * radius < clearance - 1e-9:
                bad.append(f"{e.pad.name}: via {centres - 2 * radius:.3f} mm from via of {other.pad.name}")
            other_stub = ((other.pad.x, other.pad.y), other.via)
            if _point_segment(*e.via, *other_stub[0], *other_stub[1]) - radius - half < clearance - 1e-9:
                bad.append(f"{e.pad.name}: via too close to the stub of {other.pad.name}")
    return sorted(set(bad))


# --- the rest of the board, all in board mm ---------------------------------

def copper_stack(text: str) -> list[str]:
    """The board's copper layers, top to bottom: F.Cu, In1.Cu .. InN.Cu, B.Cu."""
    match = re.search(r"\(layers\s", text)
    if not match:
        return []
    block = text[match.start():_balanced(text, match.start())]
    names = re.findall(r'\(\d+\s+"([^"]+\.Cu)"', block)
    inner = sorted((n for n in names if n.startswith("In")),
                   key=lambda n: int(re.sub(r"\D", "", n) or 0))
    return [n for n in ("F.Cu",) if n in names] + inner + [n for n in ("B.Cu",) if n in names]


def adjacent_layer(pad_layer: str, stack: list[str]) -> str | None:
    """The copper layer next to an outer layer: the one a microvia reaches."""
    if pad_layer not in stack or len(stack) < 3:
        return None
    return stack[1] if stack.index(pad_layer) == 0 else stack[-2]


@dataclass
class Obstacle:
    """Copper or a hole the escape must clear, as an axis-aligned box or a
    segment, on a set of copper layers (`None` means every layer)."""
    what: str
    net: str | None
    layers: set[str] | None
    rect: tuple[float, float, float, float] | None = None
    segment: tuple[tuple[float, float], tuple[float, float], float] | None = None


def _pad_layers(pad: str) -> set[str] | None:
    block = _children(pad, "layers")
    names = re.findall(r'"([^"]+)"', block[0]) if block else []
    if any(n in ("*.Cu", "F&B.Cu") for n in names):
        return None
    return {n for n in names if n.endswith(".Cu")}


def board_obstacles(text: str, skip: tuple[int, int]) -> list[Obstacle]:
    """Every other part's pads and holes, the free tracks and vias, and the
    Edge.Cuts outline. The escaped footprint (`skip`, its span in `text`) is
    checked separately in its own coordinates."""
    out: list[Obstacle] = []
    for start, end, block in footprints(text):
        if (start, end) == skip:
            continue
        at = _numbers(block, "at")
        if len(at) < 2:
            continue
        fp = Footprint("", at[0], at[1], at[2] if len(at) > 2 else 0.0, [])
        reference = re.search(r'\(property\s+"Reference"\s+"([^"]*)"', block)
        ref = reference.group(1) if reference else "?"
        for pad in _children(block, "pad"):
            head = re.match(r'\(pad\s+"([^"]*)"\s+(\w+)', pad)
            pad_at, size = _numbers(pad, "at"), _numbers(pad, "size")
            if not head or len(pad_at) < 2 or len(size) < 2:
                continue
            cx, cy = to_board(fp, pad_at[0], pad_at[1])
            a = math.radians(pad_at[2] if len(pad_at) > 2 else 0.0)
            # The box round the pad at its board angle: conservative for a
            # pad that is not at a right angle.
            hw = (abs(size[0] * math.cos(a)) + abs(size[1] * math.sin(a))) / 2
            hh = (abs(size[0] * math.sin(a)) + abs(size[1] * math.cos(a))) / 2
            rect = (cx - hw, cy - hh, cx + hw, cy + hh)
            net = re.search(r'\(net\s+(?:\d+\s+)?"([^"]*)"\)', pad)
            kind = head.group(2)
            if kind == "np_thru_hole":
                # A bare hole: no net to share, and it goes through every layer.
                out.append(Obstacle(f"hole {ref}", None, None, rect=rect))
                continue
            layers = None if kind == "thru_hole" else _pad_layers(pad)
            out.append(Obstacle(f"pad {ref}.{head.group(1)}", net.group(1) if net else None,
                                layers, rect=rect))
    for seg in re.finditer(r"\(segment\s", text):
        block = text[seg.start():_balanced(text, seg.start())]
        a, b, w = _numbers(block, "start"), _numbers(block, "end"), _numbers(block, "width")
        layer = re.search(r'\(layer\s+"([^"]+)"\)', block)
        net = re.search(r'\(net\s+(\d+|"[^"]*")', block)
        if len(a) >= 2 and len(b) >= 2 and w and layer:
            out.append(Obstacle("track", _net_key(net.group(1) if net else None, text), {layer.group(1)},
                                segment=((a[0], a[1]), (b[0], b[1]), w[0])))
    for via in re.finditer(r"\(via\s", text):
        block = text[via.start():_balanced(text, via.start())]
        at, size = _numbers(block, "at"), _numbers(block, "size")
        net = re.search(r'\(net\s+(\d+|"[^"]*")', block)
        if len(at) >= 2 and size:
            r = size[0] / 2
            # Its span is not resolved: every via counts on every layer.
            out.append(Obstacle("via", _net_key(net.group(1) if net else None, text), None,
                                rect=(at[0] - r, at[1] - r, at[0] + r, at[1] + r)))
    for edge in _edge_segments(text):
        out.append(Obstacle("board edge", None, None, segment=(edge[0], edge[1], 0.0)))
    return out


def _net_key(token: str | None, text: str) -> str | None:
    """A net number or quoted name, as the net's name."""
    if token is None:
        return None
    if token.startswith('"'):
        return token.strip('"')
    named = re.search(rf'\(net\s+{token}\s+"([^"]*)"\)', text)
    return named.group(1) if named else token


def _edge_segments(text: str) -> list[tuple[tuple[float, float], tuple[float, float]]]:
    """The Edge.Cuts outline as straight segments (an arc by its chords)."""
    out = []
    for match in re.finditer(r"\((gr_line|gr_rect|gr_arc|gr_poly)\s", text):
        block = text[match.start():_balanced(text, match.start())]
        if '"Edge.Cuts"' not in block:
            continue
        kind = match.group(1)
        if kind == "gr_poly":
            pts = [(float(x), float(y)) for x, y in re.findall(r"\(xy\s+([-\d.]+)\s+([-\d.]+)\)", block)]
            out.extend(zip(pts, pts[1:] + pts[:1]))
            continue
        a, b = _numbers(block, "start"), _numbers(block, "end")
        if len(a) < 2 or len(b) < 2:
            continue
        if kind == "gr_rect":
            (x0, y0), (x1, y1) = a[:2], b[:2]
            c = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            out.extend(zip(c, c[1:] + c[:1]))
        elif kind == "gr_arc":
            mid = _numbers(block, "mid")
            pts = [(a[0], a[1])] + ([(mid[0], mid[1])] if len(mid) >= 2 else []) + [(b[0], b[1])]
            out.extend(itertools.pairwise(pts))
        else:
            out.append(((a[0], a[1]), (b[0], b[1])))
    return out


def check_board(fp: Footprint, escapes: list[Escape], obstacles: list[Obstacle],
                via_size: float, stub_width: float, clearance: float,
                edge_clearance: float) -> list[str]:
    """Pads whose via or stub breaks a clearance to the rest of the board."""
    radius, half = via_size / 2, stub_width / 2
    bad: list[str] = []
    for e in escapes:
        via = to_board(fp, *e.via)
        stub = (to_board(fp, e.pad.x, e.pad.y), via)
        span = {e.pad.layer, e.target}
        for obstacle in obstacles:
            if obstacle.net is not None and obstacle.net == e.pad.net_name:
                continue
            edge = obstacle.what == "board edge"
            need = edge_clearance if edge else clearance
            on_via = obstacle.layers is None or bool(obstacle.layers & span)
            on_stub = obstacle.layers is None or e.pad.layer in obstacle.layers
            if obstacle.rect is not None:
                via_gap = _segment_rect(via, via, obstacle.rect) - radius
                stub_gap = _segment_rect(*stub, obstacle.rect) - half
            else:
                a, b, width = obstacle.segment
                via_gap = _point_segment(*via, *a, *b) - radius - width / 2
                stub_gap = _segment_segment(*stub, a, b) - half - width / 2
            if on_via and via_gap < need - 1e-9:
                bad.append(f"{e.pad.name}: via {max(via_gap, 0.0):.3f} mm from {obstacle.what}")
            if on_stub and stub_gap < need - 1e-9:
                bad.append(f"{e.pad.name}: stub {max(stub_gap, 0.0):.3f} mm from {obstacle.what}")
    return sorted(set(bad))


def _net(pad: Pad) -> str:
    return f"(net {pad.net_number})" if pad.net_number is not None else f'(net "{pad.net_name}")'


def copper(fp: Footprint, escapes: list[Escape], via_size: float, via_drill: float,
           stub_width: float) -> str:
    out = []
    for e in escapes:
        start = to_board(fp, e.pad.x, e.pad.y)
        end = to_board(fp, *e.via)
        out.append(
            f'\t(segment (start {start[0]:.4f} {start[1]:.4f}) (end {end[0]:.4f} {end[1]:.4f}) '
            f'(width {stub_width}) (layer "{e.pad.layer}") {_net(e.pad)} (uuid "{uuid.uuid4()}"))'
        )
        # One layer pair, so a laser microvia (see `adjacent_layer`).
        out.append(
            f'\t(via micro (at {end[0]:.4f} {end[1]:.4f}) (size {via_size}) (drill {via_drill}) '
            f'(layers "{e.pad.layer}" "{e.target}") {_net(e.pad)} (uuid "{uuid.uuid4()}"))'
        )
    return "\n".join(out)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("board", type=Path)
    parser.add_argument("--component", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--layers", nargs="+", default=["F.Cu", "In1.Cu", "In2.Cu"])
    parser.add_argument("--via-size", type=float, default=0.25)
    parser.add_argument("--via-drill", type=float, default=0.1)
    parser.add_argument("--stub-width", type=float, default=0.15)
    parser.add_argument("--clearance", type=float, default=0.1)
    parser.add_argument("--edge-clearance", type=float, default=0.3,
                        help="copper to Edge.Cuts, mm")
    args = parser.parse_args(argv)

    def summary(requested: int, escaped: int, failed: list[str]) -> None:
        print("JSON_SUMMARY:" + json.dumps({
            "requested": requested, "escaped": escaped, "failed": len(failed),
            "unescaped_nets": failed,
        }))

    text = args.board.read_text()
    found = [
        (start, end, fp) for start, end, block in footprints(text)
        if (fp := parse_footprint(block)) and fp.reference == args.component
    ]
    if not found:
        print(f"error: no footprint {args.component} on the board", file=sys.stderr)
        summary(0, 0, [args.component])
        return ESCAPE_FAILED
    start, end, fp = found[0]
    outer = sorted({p.layer for p in fp.pads})
    if len(outer) != 1:
        print(f"error: {args.component} has pads on {', '.join(outer) or 'no copper'}; "
              "the escape handles a one-sided connector", file=sys.stderr)
        summary(len(fp.pads), 0, [p.net_name for p in fp.pads] or [args.component])
        return ESCAPE_FAILED
    # The board's own stack decides the neighbour. A board file without a
    # layer table can only be read as F.Cu over In1.Cu.
    stack = copper_stack(text) or ["F.Cu", "In1.Cu"]
    target = adjacent_layer(outer[0], stack) if len(stack) >= 3 else (
        "In1.Cu" if outer[0] == "F.Cu" else None)
    # The pad side may be left out of the declared list; the rest are the
    # layers the escape may land on.
    allowed = [layer for layer in args.layers if layer != outer[0]]
    if target is None or target not in allowed:
        reason = (
            f"a microvia from {outer[0]} reaches {target}, which the escape's layers "
            f"({', '.join(allowed) or 'none'}) do not include"
            if target else f"the board has no copper layer next to {outer[0]} for a microvia"
        )
        print(f"error: cannot escape {args.component}: {reason}", file=sys.stderr)
        summary(len(fp.pads), 0, [p.net_name for p in fp.pads] or [args.component])
        return ESCAPE_FAILED
    escapes, bad = plan(fp, target, args.via_size, args.via_drill, args.stub_width,
                        args.clearance)
    if escapes and not bad:
        bad = check_board(fp, escapes, board_obstacles(text, (start, end)), args.via_size,
                          args.stub_width, args.clearance, args.edge_clearance)
    if bad or not escapes:
        for reason in bad:
            print(f"error: cannot escape {args.component} pad {reason}", file=sys.stderr)
        failed_pads = sorted({reason.split(":")[0] for reason in bad}) or [args.component]
        summary(len(fp.pads), 0, [
            next((p.net_name for p in fp.pads if p.name == name), name) for name in failed_pads
        ])
        return ESCAPE_FAILED
    body = copper(fp, escapes, args.via_size, args.via_drill, args.stub_width)
    close = text.rstrip().rfind(")")
    args.output.write_text(text[:close].rstrip() + "\n" + body + "\n)\n")
    print(f"connector dogbone escaped {len(escapes)} pad(s) of {args.component}")
    summary(len(escapes), len(escapes), [])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
