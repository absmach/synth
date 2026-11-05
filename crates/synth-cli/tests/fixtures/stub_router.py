#!/usr/bin/python3
# Test double for an external PCB router.
#
# Installed as `python3` on PATH by the CLI integration tests, so the
# FreeRouting adapter invokes it exactly as it invokes the real helper: as the
# interpreter that will run tools/freeroute_autoroute.py. The shebang is an
# absolute path resolved by the test harness before this file is put on PATH,
# so it cannot resolve back to itself.
#
# Its job is to return a candidate that passes the same independent checks a
# real engine's output must pass, so the tests around it measure the gate
# rather than the stub. It does that in two steps:
#
#   1. It copies the un-routed baseline to the candidate, so the candidate is
#      a genuine router output rather than a file the tests wrote themselves.
#   2. It connects the pads of each net with a straight segment per pad pair.
#      Without this the fixture designs — which do have a net — would come
#      back as an unroutable board, and a test about an override or a release
#      manifest would fail for a reason unrelated to either.
#
# This is not a router and does not try to be. What it owes the pipeline is a
# board whose copper agrees with its own netlist, so that the independent
# topology and connectivity checks downstream have something real to confirm
# or reject. It reads only the text of the board it is given.

import json
import math
import os
import re
import shutil
import sys

NM_PER_MM = 1_000_000
# Matches the fabrication minimum the validator enforces, so the stub's
# copper is not rejected for being under-width.
TRACK_WIDTH_MM = 0.25

FLAGS_WITH_VALUE = {
    "--report",
    "--ses-output",
    "--stats",
    "--passes",
    "--threads",
    "--jar",
    "--java",
    "--repo",
    "--python",
    "--escalation",
    "--fab-tier",
    "--fab-overrides",
    "--same-net-pad-clearance",
}


def parse_args(argv):
    """Return (helper, baseline, candidate, report).

    The positional order is the production contract:
      <helper> <baseline> <candidate> --jar <jar> --java <java> ...
    """
    positional = []
    report = None
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == "--report":
            report = argv[index + 1]
            index += 2
        elif arg in FLAGS_WITH_VALUE:
            index += 2
        elif arg.startswith("--"):
            index += 1
        else:
            positional.append(arg)
            index += 1
    helper = positional[0] if positional else None
    baseline = positional[1] if len(positional) > 1 else None
    candidate = positional[2] if len(positional) > 2 else None
    return helper, baseline, candidate, report


NET_TABLE = re.compile(r'^\s*\(net (\d+) "([^"]*)"')
FOOTPRINT_OPEN = re.compile(r"^\s*\(footprint\b")
AT_LINE = re.compile(r"^\s*\(at (-?[0-9.]+) (-?[0-9.]+)(?: (-?[0-9.]+))?")
PAD_START = re.compile(r'^\s*\(pad\s+"([^"]+)"')
AT_ON_LINE = re.compile(r"\(at (-?[0-9.]+) (-?[0-9.]+)")
NET_ON_LINE = re.compile(r'\(net (\d+)\s+"')


def route(baseline_text):
    """Return (segments, net_code).

    One segment per additional pad of a net, anchored on the first pad seen.
    Pad `(at ...)` is relative to its footprint, so each offset is rotated by
    the footprint's angle and translated by the footprint origin; skipping
    either step puts the segment beside the pad instead of on it.

    The exporter pretty-prints a pad across several lines — `(pad "1" ...` on
    one, `(at ...)` and `(net ...)` on later ones — so a pad is parsed as a
    little state machine rather than off a single line.
    """
    net_names = {}
    origin = None
    angle_deg = 0.0
    first_pad = {}
    seen = {}
    segments = []
    net_code = 0
    pad = None  # {"x": float|None, "y": float|None} while inside a pad block

    def place(code, local_x, local_y):
        nonlocal net_code
        if origin is None or local_x is None or code == 0:
            return
        net_code = code
        # KiCad angles are counter-clockwise on screen while y runs down the
        # page, so a positive angle turns local x toward negative y.
        radians = math.radians(angle_deg)
        x = origin[0] + local_x * math.cos(radians) + local_y * math.sin(radians)
        y = origin[1] - local_x * math.sin(radians) + local_y * math.cos(radians)
        key = net_names.get(str(code), str(code))
        seen[key] = seen.get(key, 0) + 1
        if seen[key] == 1:
            first_pad[key] = (x, y)
        else:
            segments.append((first_pad[key], (x, y), code))

    for line in baseline_text.splitlines():
        # Pad handling comes first: a pad's own `(net N "…")` line starts like
        # a top-level net declaration, so the pad state machine must claim it
        # before the net table does.
        if PAD_START.match(line):
            pad = {"x": None, "y": None}
            match = AT_ON_LINE.search(line)
            if match:
                pad["x"], pad["y"] = float(match.group(1)), float(match.group(2))
            match = NET_ON_LINE.search(line)
            if match:
                place(int(match.group(1)), pad["x"], pad["y"])
                pad = None
            continue

        if pad is not None:
            if pad["x"] is None:
                match = AT_ON_LINE.search(line)
                if match:
                    pad["x"], pad["y"] = float(match.group(1)), float(match.group(2))
                    continue
            match = NET_ON_LINE.search(line)
            if match:
                place(int(match.group(1)), pad["x"], pad["y"])
                pad = None
            continue

        match = NET_TABLE.match(line)
        if match:
            net_names[match.group(1)] = match.group(2)
            continue

        if FOOTPRINT_OPEN.match(line):
            origin, angle_deg, pad = None, 0.0, None
            continue

        # The footprint origin is the first `(at ...)` in the footprint block
        # that is not inside a pad; property effects carry their own `(at ...)`
        # but follow the origin.
        if origin is None and line.lstrip().startswith("(at"):
            match = AT_LINE.match(line)
            if match:
                origin = (float(match.group(1)), float(match.group(2)))
                angle_deg = float(match.group(3)) if match.group(3) else 0.0

    return segments, net_code


def splice(board_text, segments):
    """Insert `segments` before the board's closing paren.

    The root list ends with the final `)` in the file. Appending after it would
    leave two top-level s-expressions, which is not a `.kicad_pcb` at all
    rather than a board carrying extra copper.
    """
    if not segments:
        return board_text
    lines = board_text.rstrip("\n").split("\n")
    if not lines or not lines[-1].strip().startswith(")"):
        return board_text
    body, closer = lines[:-1], lines[-1]
    rendered = [
        '  (segment (start {:.4f} {:.4f}) (end {:.4f} {:.4f}) (width {}) (layer "F.Cu") (net {}))'.format(
            start[0], start[1], end[0], end[1], TRACK_WIDTH_MM, code
        )
        for start, end, code in segments
    ]
    if os.environ.get("STUB_ROUTER_SILK_ON_PAD"):
        # Opt-in: print a silk stroke across the first pad, so a test can
        # exercise the manufacturability gate on a board that really has it.
        x, y = segments[0][0]
        rendered.append(
            '  (gr_line (start {:.4f} {:.4f}) (end {:.4f} {:.4f}) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))'.format(
                x, y, x + 0.5, y
            )
        )
    return "\n".join(body + rendered + [closer]) + "\n"


# The interpreter this file stands in for, baked in by the test harness when
# it installs the stub. A stub that cannot answer `-c` questions is not a
# faithful stand-in, and the router pipeline legitimately asks its
# interpreter whether KiCad's bindings are importable before routing.
REAL_PYTHON = "@@REAL_PYTHON@@"


def delegate_to_real_python(argv) -> None:
    """Hand an interpreter-style invocation to the real Python.

    The stub is installed *as* `python3`, so anything that is not a routing
    run is the pipeline talking to its interpreter and must be answered by
    one. Delegating keeps the stub from having to reimplement Python.
    """
    os.execv(REAL_PYTHON, [REAL_PYTHON, *argv])


def looks_like_a_routing_run(argv) -> bool:
    """Whether these arguments are a router invocation rather than a probe.

    A routing run names a helper script and then a board to read and a board
    to write. Everything else — `-c`, `-m`, `-V`, or nothing at all — is the
    interpreter being used as an interpreter.
    """
    positional = []
    index = 0
    while index < len(argv):
        arg = argv[index]
        if arg == "--report" or arg in FLAGS_WITH_VALUE:
            index += 2
        elif arg.startswith("--"):
            index += 1
        elif arg in ("-c", "-m", "-V", "--version", "-"):
            return False
        else:
            positional.append(arg)
            index += 1
    return len(positional) >= 3 and positional[0].endswith(".py")


def main(argv):
    if not looks_like_a_routing_run(argv):
        delegate_to_real_python(argv)
    _helper, baseline, candidate, report = parse_args(argv)
    print(
        "stub-router: baseline={} candidate={}".format(baseline, candidate),
        file=sys.stderr,
    )

    if not baseline or not os.path.isfile(baseline):
        print(
            "stub-router: the router was not handed the un-routed baseline at "
            "{}".format(baseline),
            file=sys.stderr,
        )
        return 2

    if report is not None:
        with open(report, "w") as handle:
            json.dump(
                {
                    "router": "stub",
                    "jar": "stub-1.0.jar",
                    "java_version": "stub-jvm",
                    "imported_segments": 0,
                    "imported_vias": 0,
                    "notes": [],
                },
                handle,
            )

    with open(baseline) as handle:
        board_text = handle.read()
    segments, net_code = route(board_text)
    with open(candidate, "w") as handle:
        handle.write(splice(board_text, segments))

    if report is not None and segments:
        # The provenance record claims what was actually imported, so the
        # validator's agreement check has something truthful to compare.
        with open(report, "w") as handle:
            json.dump(
                {
                    "router": "stub",
                    "jar": "stub-1.0.jar",
                    "java_version": "stub-jvm",
                    "imported_segments": len(segments),
                    "imported_vias": 0,
                    "notes": [],
                },
                handle,
            )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
