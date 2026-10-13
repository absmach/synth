#!/usr/bin/env python3
"""Run KiCadRoutingTools as Synth's optional external router.

KiCadRoutingTools remains an external checkout; this adapter provides a
stable scriptable entry point without vendoring its Python or native Rust
runtime.

The policy checks here are the engine's *self-assessment*, recorded so the
run can say what the router believed. They are not the verdict: the
adapter re-derives connectivity from the copper it wrote and runs
`kicad-cli pcb drc`, so a router that miscounts its own success cannot
promote itself.

Exit status is therefore deliberately narrow:

* `0` — the engine ran and its own policy gate passed. Says nothing about
  whether the board is actually routable.
* non-zero — the engine failed, or its own policy gate rejected the result.

A malformed summary is a failure rather than a shrug. The previous
behaviour warned and carried on, which meant a KRT build that emitted
unparseable JSON could produce a board that no check had ever read.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

# Exit code used when the engine itself succeeded but its own policy gate
# rejected the result. Distinct from a crash so a caller can tell "the
# router refused this board" from "the router broke".
POLICY_REJECTED = 3


def _fanout(python: str, repo: Path, board: Path, components: list[str],
            method: str, clearance: float, via_size: float | None,
            via_drill: float | None, fab_tier: str, strict_sizes: bool,
            escalation: str) -> Path:
    """Escape every named package, returning the board to route next.

    Two stages, in the order KiCadRoutingTools documents (#562): pour the
    planes first so the fanout's plane-drop vias land on real fill, then fan
    out each package. Routing before either leaves a dense package's balls to
    the general-purpose router, which is how a 0.8 mm grid ends up with a
    stub on every lane.

    A stage that fails is not fatal. The un-fanned board still routes, just
    worse, and reporting that honestly beats refusing to route at all — the
    caller sees the stage's own error and the stats still describe whatever
    copper came back.

    The fabrication floor travels with every stage. `--fab-tier auto` is
    documented to escalate to an advanced tier when a fine-pitch escape needs
    one, which is the right default for a human at a terminal and the wrong one
    for a board that has already named its manufacturer: measured without these
    flags, escaping a 121-ball BGA rewrote the project's own minimums from
    0.56/0.3 down to 0.35/0.2 and placed 11 vias there.
    """
    planes = repo / "py_router" / "route_planes.py"
    fanout = repo / "py_router" / "bga_fanout.py"
    current = board

    if planes.is_file():
        poured = board.with_suffix(".poured.kicad_pcb")
        command = [python, str(planes), str(current), str(poured)]
        print("pouring planes:", " ".join(command), flush=True)
        if subprocess.run(command, check=False).returncode == 0 and poured.is_file():
            current = poured
        else:
            print("warning: plane pour failed; fanning out on unpoured fill", file=sys.stderr)

    if not fanout.is_file():
        print(f"warning: no bga_fanout.py under {repo}; skipping escape", file=sys.stderr)
        return current

    for index, refdes in enumerate(components):
        fanned = board.with_suffix(f".fanned{index}.kicad_pcb")
        command = [
            python, str(fanout), str(current),
            "--component", refdes,
            "--output", str(fanned),
            "--escape-method", method,
            "--clearance", str(clearance),
        ]
        # The escape via is the one the net class declared, so a package that
        # needs the tier's smallest via escapes with it rather than with the
        # engine's default.
        if via_size and via_drill:
            command.extend(("--via-size", str(via_size), "--via-drill", str(via_drill)))
        command.extend(("--fab-tier", fab_tier, "--escalation", escalation))
        if strict_sizes:
            command.append("--strict-sizes")
        print(f"escaping {refdes}:", " ".join(command), flush=True)
        if subprocess.run(command, check=False).returncode == 0 and fanned.is_file():
            current = fanned
        else:
            print(f"warning: fanout of {refdes} failed; routing it unescaped", file=sys.stderr)

    return current


def _load_summary(stats: Path) -> dict:
    """Read the engine's JSON summary, or explain why it could not be read."""
    if not stats.is_file():
        raise RuntimeError(f"KRT did not write a JSON report at {stats}")
    try:
        return json.loads(stats.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"could not parse the KRT summary at {stats}: {error}") from error


def _policy_findings(report: dict, allow_via_in_pad: bool) -> list[str]:
    """Everything the engine's own summary says is wrong with the route."""
    findings: list[str] = []

    open_items = sum(
        len(report.get(key, []) or [])
        for key in ("failed_single", "open_single", "failed_multipoint")
    )
    if open_items:
        findings.append(f"{open_items} open/failed connection group(s)")

    design_rules = report.get("design_rules") or {}
    floors = design_rules.get("board_floors") or {}
    delivered = design_rules.get("min_delivered") or {}
    below = [
        f"{key} {delivered[key]:.4f} < {floor:.4f}"
        for key, floor in floors.items()
        if key in delivered and delivered[key] < floor - 1e-9
    ]
    if below:
        findings.append("delivered below board floors: " + ", ".join(sorted(below)))

    via_in_pad = (report.get("via_in_pad") or {}).get("count", 0)
    if via_in_pad and not allow_via_in_pad:
        findings.append(f"{via_in_pad} via-in-pad site(s)")

    return findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_board", type=Path)
    parser.add_argument("output_board", type=Path)
    parser.add_argument(
        "--repo", required=True, type=Path, help="KiCadRoutingTools checkout"
    )
    parser.add_argument(
        "--python",
        default=sys.executable,
        help="Python interpreter with KRT dependencies",
    )
    parser.add_argument(
        "--ordering", choices=("mps", "inside_out", "original", "bus"), default="mps"
    )
    parser.add_argument("--stats", type=Path, help="JSON route summary output")
    parser.add_argument(
        "--no-write-fill", action="store_true", help="Do not write filled zone polygons"
    )
    parser.add_argument(
        "--escalation", choices=("off", "board", "fab"), default="board"
    )
    parser.add_argument(
        "--fab-tier", choices=("standard", "advanced", "auto"), default="auto"
    )
    parser.add_argument("--fab-overrides", type=Path)
    parser.add_argument("--same-net-pad-clearance", type=float, default=0.1)
    parser.add_argument("--strict-sizes", action="store_true", default=True)
    parser.add_argument("--allow-via-in-pad", action="store_true")
    parser.add_argument(
        "--fanout-component",
        action="append",
        default=[],
        metavar="REFDES",
        help="Escape this package before routing. Repeatable. Synth passes the "
        "packages its own escape analysis found cannot be fanned out at the "
        "fabrication floor.",
    )
    parser.add_argument(
        "--escape-method",
        default="underpad",
        help="Escape engine for --fanout-component (KRT: channel or underpad). "
        "underpad is the one that escapes fully populated arrays.",
    )
    parser.add_argument("--escape-clearance", type=float, default=0.1)
    parser.add_argument("--escape-via-size", type=float)
    parser.add_argument("--escape-via-drill", type=float)
    args = parser.parse_args()

    route = args.repo / "py_router" / "route.py"
    if not route.is_file():
        parser.error(f"KRT route.py not found under {args.repo}")
    args.output_board.parent.mkdir(parents=True, exist_ok=True)
    stats = args.stats or args.output_board.with_suffix(".krt-stats.json")
    board_to_route = args.input_board
    if args.fanout_component:
        board_to_route = _fanout(
            args.python,
            args.repo,
            args.input_board,
            args.fanout_component,
            args.escape_method,
            args.escape_clearance,
            args.escape_via_size,
            args.escape_via_drill,
            args.fab_tier,
            args.strict_sizes,
            args.escalation,
        )
    command = [
        args.python,
        str(route),
        str(board_to_route),
        str(args.output_board),
        "--nets",
        "*",
        "--ordering",
        args.ordering,
        "--stats",
        "--json-out",
        str(stats),
        "--escalation",
        args.escalation,
        "--same-net-pad-clearance",
        str(-1 if args.allow_via_in_pad else args.same_net_pad_clearance),
        "--fab-tier",
        args.fab_tier,
    ]
    if args.fab_overrides:
        command.extend(("--fab-overrides", str(args.fab_overrides)))
    if args.strict_sizes:
        command.append("--strict-sizes")
    if not args.no_write_fill:
        command.append("--write-fill")

    print("running KiCadRoutingTools:", " ".join(command), flush=True)
    completed = subprocess.run(command, check=False)

    policy_ok = completed.returncode == 0
    if completed.returncode:
        print(f"error: KiCadRoutingTools exited {completed.returncode}", file=sys.stderr)

    try:
        report = _load_summary(stats)
    except RuntimeError as error:
        print(f"error: {error}", file=sys.stderr)
        return completed.returncode or POLICY_REJECTED

    print(
        f"KRT summary: routed={report.get('successful', '?')} "
        f"failed={report.get('failed', '?')} "
        f"vias={report.get('total_vias', '?')} "
        f"time={report.get('total_time', '?')}s",
        flush=True,
    )

    for finding in _policy_findings(report, args.allow_via_in_pad):
        print(f"error: KiCadRoutingTools policy: {finding}", file=sys.stderr)
        policy_ok = False

    if not args.output_board.is_file():
        print("error: KRT produced no output board", file=sys.stderr)
        return completed.returncode or POLICY_REJECTED

    # The engine's own connectivity checker is a useful cross-check, but a
    # missing checker must not silently skip the gate: say so instead.
    checker = args.repo / "py_router" / "check_connected.py"
    if checker.is_file():
        connected = subprocess.run(
            [args.python, str(checker), str(args.output_board), "--quiet"], check=False
        )
        if connected.returncode:
            print(
                "error: KRT connectivity checker reported disconnected routes",
                file=sys.stderr,
            )
            policy_ok = False
    else:
        print(
            f"warning: {checker} is absent, so KRT's own connectivity check was "
            "skipped; connectivity is re-derived downstream",
            file=sys.stderr,
        )

    return 0 if policy_ok else (completed.returncode or POLICY_REJECTED)


if __name__ == "__main__":
    raise SystemExit(main())