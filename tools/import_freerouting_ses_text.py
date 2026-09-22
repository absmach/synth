#!/usr/bin/env python3
"""Merge FreeRouting SES wire/via records into a KiCad board textually."""

from __future__ import annotations

import argparse
import re
import uuid
from pathlib import Path


def tokens(text: str):
    token = ""
    quoted = False
    escaped = False
    for char in text:
        if quoted:
            if escaped:
                token += char
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
                yield token
                token = ""
            else:
                token += char
        elif char == '"':
            if token:
                yield token
                token = ""
            quoted = True
        elif char in "()":
            if token:
                yield token
                token = ""
            yield char
        elif char.isspace():
            if token:
                yield token
                token = ""
        else:
            token += char
    if token:
        yield token


def parse(text: str):
    stack = []
    root = None
    for token in tokens(text):
        if token == "(":
            node = []
            if stack:
                stack[-1].append(node)
            stack.append(node)
        elif token == ")":
            root = stack.pop()
        else:
            stack[-1].append(token)
    return root


def children(node, name):
    return [x for x in node[1:] if isinstance(x, list) and x and x[0] == name]


def coord(value: str) -> float:
    return int(value) / 10000.0


def xy(x: str, y: str) -> tuple[float, float]:
    return coord(x), -coord(y)


def fmt(value: float) -> str:
    return f"{value:.4f}".rstrip("0").rstrip(".")


def top_level_blocks(text: str):
    depth = 0
    quoted = False
    escaped = False
    start = None
    for index, char in enumerate(text):
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
            continue
        if char == '"':
            quoted = True
        elif char == "(":
            if depth == 1:
                start = index
            depth += 1
        elif char == ")":
            depth -= 1
            if depth == 1 and start is not None:
                yield start, index + 1, text[start:index + 1]
                start = None


def board_net_codes(text: str) -> dict[str, str]:
    return {
        name: code
        for code, name in re.findall(r'\(net\s+(\d+)\s+"([^"]*)"\)', text)
    }


def route_records(ses_text: str, net_codes: dict[str, str]):
    root = parse(ses_text)
    routes = next(x for x in root if isinstance(x, list) and x and x[0] == "routes")
    network = next(x for x in routes if isinstance(x, list) and x and x[0] == "network_out")
    records = []
    for net in children(network, "net"):
        net_code = net_codes[net[1]]
        for wire in children(net, "wire"):
            for path in children(wire, "path"):
                layer, width = path[1], fmt(max(coord(path[2]), 0.127))
                points = [xy(path[i], path[i + 1]) for i in range(3, len(path), 2)]
                for first, second in zip(points, points[1:]):
                    records.append(
                        "\t(segment\n"
                        f"\t\t(start {fmt(first[0])} {fmt(first[1])})\n"
                        f"\t\t(end {fmt(second[0])} {fmt(second[1])})\n"
                        f"\t\t(width {width})\n"
                        f"\t\t(layer \"{layer}\")\n"
                        f"\t\t(net {net_code})\n"
                        f"\t\t(uuid \"{uuid.uuid4()}\")\n"
                        "\t)\n"
                    )
        for via in children(net, "via"):
            name, x, y = via[1:4]
            diameter, drill = ("0.8", "0.4") if "800:400" in name else ("0.6", "0.3")
            px, py = xy(x, y)
            records.append(
                "\t(via\n"
                f"\t\t(at {fmt(px)} {fmt(py)})\n"
                f"\t\t(size {diameter})\n"
                f"\t\t(drill {drill})\n"
                "\t\t(layers \"F.Cu\" \"B.Cu\")\n"
                f"\t\t(net {net_code})\n"
                f"\t\t(uuid \"{uuid.uuid4()}\")\n"
                "\t)\n"
            )
    return records


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("input_board")
    parser.add_argument("input_ses")
    parser.add_argument("output_board")
    args = parser.parse_args()

    board = Path(args.input_board).read_text()
    net_codes = board_net_codes(board)
    records = route_records(Path(args.input_ses).read_text(), net_codes)
    spans = list(top_level_blocks(board))
    kept = []
    insert_at = None
    for start, end, block in spans:
        first = block.lstrip()[1:].lstrip().split(None, 1)[0]
        if first in {"segment", "via"}:
            continue
        if insert_at is None and first == "zone":
            insert_at = start
        kept.append((start, end, block))
    if insert_at is None:
        insert_at = board.rfind(")")
    # Remove old route blocks while preserving every other top-level object.
    rebuilt = []
    cursor = 0
    for start, end, block in spans:
        if block.lstrip()[1:].lstrip().split(None, 1)[0] in {"segment", "via"}:
            rebuilt.append(board[cursor:start])
            cursor = end
    rebuilt.append(board[cursor:])
    without_routes = "".join(rebuilt)
    insert_at = without_routes.find("\n\t(zone")
    if insert_at < 0:
        insert_at = without_routes.rfind(")")
    result = without_routes[:insert_at] + "\n" + "".join(records) + without_routes[insert_at:]
    Path(args.output_board).write_text(result)
    print(f"merged {len(records)} FreeRouting records")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
