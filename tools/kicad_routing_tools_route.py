#!/usr/bin/env python3
"""Run KiCadRoutingTools as an optional Synth external post-router.

KiCadRoutingTools remains an external checkout; this adapter provides a stable
scriptable entry point without vendoring its Python or native Rust runtime.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_board", type=Path)
    parser.add_argument("output_board", type=Path)
    parser.add_argument("--repo", required=True, type=Path, help="KiCadRoutingTools checkout")
    parser.add_argument("--python", default=sys.executable, help="Python interpreter with KRT dependencies")
    parser.add_argument("--ordering", choices=("mps", "inside_out", "original", "bus"), default="mps")
    parser.add_argument("--stats", type=Path, help="JSON route summary output")
    parser.add_argument("--no-write-fill", action="store_true", help="Do not write filled zone polygons")
    parser.add_argument("--escalation", choices=("off", "board", "fab"), default="board")
    parser.add_argument("--fab-tier", choices=("standard", "advanced", "auto"), default="auto")
    parser.add_argument("--fab-overrides", type=Path)
    parser.add_argument("--same-net-pad-clearance", type=float, default=0.1)
    parser.add_argument("--strict-sizes", action="store_true", default=True)
    parser.add_argument("--allow-via-in-pad", action="store_true")
    args = parser.parse_args()

    route = args.repo / "py_router" / "route.py"
    if not route.is_file():
        parser.error(f"KRT route.py not found under {args.repo}")
    args.output_board.parent.mkdir(parents=True, exist_ok=True)
    stats = args.stats or args.output_board.with_suffix(".krt-stats.json")
    command = [
        args.python, str(route), str(args.input_board), str(args.output_board),
        "--nets", "*", "--ordering", args.ordering, "--stats", "--json-out", str(stats),
        "--escalation", args.escalation, "--same-net-pad-clearance",
        str(-1 if args.allow_via_in_pad else args.same_net_pad_clearance),
        "--fab-tier", args.fab_tier,
    ]
    if args.fab_overrides:
        command.extend(("--fab-overrides", str(args.fab_overrides)))
    if args.strict_sizes:
        command.append("--strict-sizes")
    if not args.no_write_fill:
        command.append("--write-fill")
    print("running KiCadRoutingTools:", " ".join(command))
    completed = subprocess.run(command, check=False)
    policy_ok = completed.returncode == 0
    report = None
    if stats.is_file():
        try:
            report = json.loads(stats.read_text())
            print(f"KRT summary: routed={report.get('successful', '?')} failed={report.get('failed', '?')} vias={report.get('total_vias', '?')} time={report.get('total_time', '?')}s")
        except (OSError, json.JSONDecodeError):
            print(f"warning: could not parse KRT stats at {stats}", file=sys.stderr)
    else:
        print(f"error: KRT did not write a JSON report at {stats}", file=sys.stderr)
        policy_ok = False

    if report is not None:
        open_items = sum(len(report.get(key, [])) for key in ("failed_single", "open_single", "failed_multipoint"))
        design_rules = report.get("design_rules", {})
        floors = design_rules.get("board_floors", {})
        delivered = design_rules.get("min_delivered", {})
        below_floor = [key for key, floor in floors.items() if key in delivered and delivered[key] < floor - 1e-9]
        via_in_pad = report.get("via_in_pad", {}).get("count", 0)
        if open_items:
            print(f"error: KRT report contains {open_items} open/failed connection groups", file=sys.stderr)
            policy_ok = False
        if below_floor:
            print(f"error: KRT delivered below board floors: {', '.join(below_floor)}", file=sys.stderr)
            policy_ok = False
        if via_in_pad and not args.allow_via_in_pad:
            print(f"error: KRT report contains {via_in_pad} via-in-pad sites", file=sys.stderr)
            policy_ok = False

    checker = args.repo / "py_router" / "check_connected.py"
    if args.output_board.is_file() and checker.is_file():
        connected = subprocess.run(
            [args.python, str(checker), str(args.output_board), "--quiet"], check=False
        )
        if connected.returncode:
            print("error: KRT connectivity checker reported disconnected routes", file=sys.stderr)
            policy_ok = False
    elif not args.output_board.is_file():
        print("error: KRT produced no output board", file=sys.stderr)
        policy_ok = False
    return 0 if policy_ok else (completed.returncode or 3)


if __name__ == "__main__":
    raise SystemExit(main())
