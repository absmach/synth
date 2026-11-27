#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Connector-dogbone escape for a two-row fine-pitch connector (absmach/synth#433).

A 0.4 mm-pitch board-to-board connector (the CM4's DF40: 0.2 x 0.7 mm pads,
two rows 3.08 mm apart) leaves no room for a standard via at a pad or between
two pads of a row. It leaves a channel between the rows. This helper gives
every netted SMD pad of the named footprint a short stub into that channel and
a microvia at its end, on a staggered grid: alternate pads of a row put their
via one via pitch further in, so neighbouring vias and the stubs that pass
them keep their clearance. Alternate vias also land on alternate inner layers
from the declared list, spreading the escapes.

Every clearance the issue asks for is checked on the geometry written, not
assumed from the grid: no via drill touches an SMD pad, and via-to-pad,
via-to-track and via-to-via clearance between different nets is at least
`--clearance`. A pad that cannot be escaped fails the helper with its name;
nothing is dropped silently. The board is written only when every pad escaped.

The last line printed is the `JSON_SUMMARY:` ledger the routing helper reads,
in bga_fanout's shape: `requested`, `escaped`, `failed`, `unescaped_nets`.
"""

from __future__ import annotations

import argparse
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


def plan(fp: Footprint, layers: list[str], via_size: float, via_drill: float,
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
            # Alternate lines land on alternate inner layers.
            inner = layers[1:]
            escapes.append(Escape(pad, via, inner[line % len(inner)]))
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
        adjacent = (e.pad.layer, e.target) in (("F.Cu", "In1.Cu"),)
        kind = "micro" if adjacent else "blind"
        out.append(
            f'\t(via {kind} (at {end[0]:.4f} {end[1]:.4f}) (size {via_size}) (drill {via_drill}) '
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
    _, _, fp = found[0]
    if len(args.layers) < 2:
        print("error: the escape needs the pad layer and at least one inner layer",
              file=sys.stderr)
        summary(len(fp.pads), 0, [p.name for p in fp.pads])
        return ESCAPE_FAILED
    layers = args.layers
    outer = {p.layer for p in fp.pads}
    if layers[0] not in outer:
        layers = [next(iter(outer)), *[l for l in layers if l not in outer]]
    escapes, bad = plan(fp, layers, args.via_size, args.via_drill, args.stub_width,
                        args.clearance)
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
