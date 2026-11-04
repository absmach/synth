#!/usr/bin/env python3
"""Run: python3 tools/test_kicad_routing_tools_route.py

The adapter routes at a high via cost first and again at the engine's default
when the first attempt leaves anything open. A controlled stub stands in for
KiCadRoutingTools: it logs each invocation and answers with the next summary
from a script.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ADAPTER = Path(__file__).parent / "kicad_routing_tools_route.py"

STUB = """\\
import json, sys
from pathlib import Path

here = Path(__file__).parent
calls = here / "calls.log"
count = len(calls.read_text().splitlines()) if calls.exists() else 0
with calls.open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
plan = json.loads((here / "summaries.json").read_text())
step = plan[min(count, len(plan) - 1)]
if not isinstance(step, dict) or "summary" not in step:
    step = {"summary": step}
if step["summary"] is not None:
    Path(sys.argv[sys.argv.index("--json-out") + 1]).write_text(json.dumps(step["summary"]))
if step.get("board", True):
    Path(sys.argv[2]).write_text("(kicad_pcb)")
sys.exit(step.get("exit", 0))
"""


def run_adapter(summaries):
    """Run the adapter against the stub; return (exit code, via costs requested).

    Each step is a summary, or `{"summary": ..., "exit": n, "board": bool}`;
    a `None` summary means the attempt writes no report, `board: False` no board."""
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "krt"
        (repo / "py_router").mkdir(parents=True)
        (repo / "py_router" / "route.py").write_text(STUB)
        (repo / "py_router" / "summaries.json").write_text(json.dumps(summaries))
        board = Path(tmp) / "in.kicad_pcb"
        board.write_text("(kicad_pcb)")
        done = subprocess.run(
            [
                sys.executable,
                str(ADAPTER),
                str(board),
                str(Path(tmp) / "out.kicad_pcb"),
                "--repo",
                str(repo),
                "--python",
                sys.executable,
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        calls = (repo / "py_router" / "calls.log").read_text().splitlines()
        costs = [
            line.split("--via-cost ")[1].split()[0] if "--via-cost" in line else "default"
            for line in calls
        ]
        return done.returncode, costs


CLEAN = {"successful": 2, "failed": 0}


QFN_STUB = """\\
import json, sys
from pathlib import Path

here = Path(__file__).parent
with (here / "perimeter.log").open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
plan = json.loads((here / "perimeter.json").read_text()) if (here / "perimeter.json").exists() else {}
if plan.get("exit", 0):
    sys.exit(plan["exit"])
out = Path(sys.argv[sys.argv.index("--output") + 1])
out.write_text(Path(sys.argv[1]).read_text() + "(perimeter-stubs)")
print("JSON_SUMMARY: " + json.dumps({
    "requested": plan.get("requested", 17), "escaped": plan.get("escaped", 17),
    "failed": plan.get("failed", 0), "unescaped_nets": []}))
"""


def run_perimeter(plan=None, summaries=None):
    """Run the adapter with one perimeter package; return (exit code, last route input, qfn calls)."""
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "krt"
        (repo / "py_router").mkdir(parents=True)
        (repo / "py_router" / "route.py").write_text(STUB)
        (repo / "py_router" / "qfn_fanout.py").write_text(QFN_STUB)
        (repo / "py_router" / "summaries.json").write_text(
            json.dumps(summaries or [{"summary": CLEAN}]))
        if plan is not None:
            (repo / "py_router" / "perimeter.json").write_text(json.dumps(plan))
        board = Path(tmp) / "in.kicad_pcb"
        board.write_text("(kicad_pcb)")
        done = subprocess.run(
            [sys.executable, str(ADAPTER), str(board), str(Path(tmp) / "out.kicad_pcb"),
             "--repo", str(repo), "--python", sys.executable,
             "--perimeter-component", "U2"],
            capture_output=True, text=True, check=False,
        )
        routed = (repo / "py_router" / "calls.log").read_text().splitlines()[-1].split()[0]
        log = repo / "py_router" / "perimeter.log"
        return done.returncode, Path(routed).name, log.read_text().splitlines() if log.exists() else []


FAILED = {"summary": {"successful": 1, "failed": 1}}


class PerimeterTest(unittest.TestCase):
    def test_a_board_that_routes_plainly_is_never_fanned_out(self):
        code, routed, calls = run_perimeter()
        self.assertEqual(code, 0)
        self.assertEqual(calls, [], "the stubs must not touch a board that routed")
        self.assertEqual(routed, "in.kicad_pcb")

    def test_a_board_that_fails_plainly_is_routed_again_with_the_stubs(self):
        code, routed, calls = run_perimeter(summaries=[FAILED, FAILED, {"summary": CLEAN}])
        self.assertEqual(code, 0)
        self.assertEqual(len(calls), 1)
        self.assertIn("--component U2", calls[0])
        self.assertNotEqual(routed, "in.kicad_pcb", "the retry must route the fanned board")

    def test_the_stage_leaves_the_projects_clearances_alone(self):
        _, _, calls = run_perimeter(summaries=[FAILED, FAILED, {"summary": CLEAN}])
        self.assertIn("--no-fix-drc-settings", calls[0])

    def test_a_perimeter_fanout_that_fails_leaves_the_failed_route_as_it_was(self):
        code, routed, calls = run_perimeter({"exit": 1}, summaries=[FAILED])
        self.assertNotEqual(code, 0)
        self.assertEqual(len(calls), 1)
        self.assertEqual(routed, "in.kicad_pcb")


class RetryTest(unittest.TestCase):
    def test_a_clean_high_via_cost_attempt_is_not_retried(self):
        code, costs = run_adapter([CLEAN])
        self.assertEqual(costs, ["1000"])
        self.assertEqual(code, 0)

    def test_a_listed_open_group_retries_at_the_default_cost(self):
        code, costs = run_adapter(
            [{"successful": 1, "failed": 1, "failed_single": ["net_3"]}, CLEAN]
        )
        self.assertEqual(costs, ["1000", "default"])
        self.assertEqual(code, 0)

    def test_a_count_only_failure_retries_at_the_default_cost(self):
        code, costs = run_adapter([{"successful": 0, "failed": 1}, CLEAN])
        self.assertEqual(costs, ["1000", "default"])
        self.assertEqual(code, 0)

    def test_a_count_only_failure_that_persists_is_rejected(self):
        code, costs = run_adapter([{"successful": 0, "failed": 1}])
        self.assertEqual(costs, ["1000", "default"])
        self.assertNotEqual(code, 0)

    def test_a_failed_high_cost_attempt_retries_and_the_retry_is_judged(self):
        code, costs = run_adapter([{"summary": CLEAN, "exit": 1}, CLEAN])
        self.assertEqual(costs, ["1000", "default"])
        self.assertEqual(code, 0)

    def test_a_retry_without_a_fresh_summary_is_not_judged_on_the_first_one(self):
        code, costs = run_adapter(
            [{"summary": {"successful": 0, "failed": 1}}, {"summary": None}]
        )
        self.assertEqual(costs, ["1000", "default"])
        self.assertNotEqual(code, 0)

    def test_a_retry_that_follows_a_failed_attempt_needs_its_own_summary(self):
        code, costs = run_adapter([{"summary": CLEAN, "exit": 1}, {"summary": None}])
        self.assertEqual(costs, ["1000", "default"])
        self.assertNotEqual(code, 0)

    def test_a_retry_without_a_fresh_board_is_not_judged_on_the_first_one(self):
        code, costs = run_adapter(
            [
                {"summary": {"successful": 0, "failed": 1}},
                {"summary": CLEAN, "board": False},
            ]
        )
        self.assertEqual(costs, ["1000", "default"])
        self.assertNotEqual(code, 0)


FANOUT_STUB = """\
import json, sys
from pathlib import Path

here = Path(__file__).parent
with (here / "fanout.log").open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
plan = json.loads((here / "fanout.json").read_text()) if (here / "fanout.json").exists() else {}
component = sys.argv[sys.argv.index("--component") + 1]
names = [f"{component}_N{i}" for i in range(plan.get("copper", 1))]
copper = "".join(
    f'(segment (start 0 0) (end 1 1) (width 0.2) (layer "F.Cu") '
    f'(net "{name}") (uuid 00000000-0000-0000-0000-{i:012d}))'
    for i, name in enumerate(names)
)
source = Path(sys.argv[1]).read_text()
Path(sys.argv[sys.argv.index("--output") + 1]).write_text(
    source.replace("(kicad_pcb)", "(kicad_pcb) " + copper, 1)
)
if plan.get("summary", True):
    print("JSON_SUMMARY: " + json.dumps({
        "requested": plan.get("requested", 1),
        "escaped": plan.get("escaped", 1),
        "failed": plan.get("failed", 0),
        "unescaped_nets": plan.get("unescaped_nets", []),
    }))
"""


def load_adapter():
    import importlib.util

    spec = importlib.util.spec_from_file_location("krt_adapter", ADAPTER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_fanout(components, required, allow_via_in_pad=False, plan=None):
    """Call `_fanout` against a stub engine.

    Returns (board routed next, escapes run, that board's text), so a test can
    assert on the copper the route would hand to the general router."""
    adapter = load_adapter()
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "krt"
        (repo / "py_router").mkdir(parents=True)
        (repo / "py_router" / "bga_fanout.py").write_text(FANOUT_STUB)
        if plan is not None:
            (repo / "py_router" / "fanout.json").write_text(json.dumps(plan))
        board = Path(tmp) / "in.kicad_pcb"
        board.write_text("(kicad_pcb)")
        routed = adapter._fanout(
            sys.executable, repo, board, components, "underpad", 0.127, None, None,
            "standard", False, "none", allow_via_in_pad, set(required),
        )
        log = repo / "py_router" / "fanout.log"
        calls = log.read_text().splitlines() if log.exists() else []
        return routed.name, [
            (line.split("--component ")[1].split()[0],
             line.split("--escape-method ")[1].split()[0])
            for line in calls
        ], routed.read_text()


class FanoutTest(unittest.TestCase):
    def test_a_declared_channel_escape_runs_without_via_in_pad(self):
        # U1 takes the under-pad default and falls back to channel (no via in a
        # pad needed); U2 asked for channel. Both escape, nothing fails.
        routed, escapes, _ = run_fanout(["U1", "U2:channel:"], required=["U2"])
        self.assertEqual(escapes, [("U1", "channel"), ("U2", "channel")])
        self.assertNotEqual(routed, "in.kicad_pcb")

    def test_a_declared_underpad_escape_without_via_in_pad_fails_the_route(self):
        with self.assertRaises(RuntimeError):
            run_fanout(["U1:underpad:"], required=["U1"])

    def test_nothing_to_escape_leaves_the_board_untouched(self):
        # With under-pad unavailable, an inferred grid falls back to channel
        # escape rather than leaving the board untouched.
        routed, escapes, output = run_fanout(["U1"], required=[])
        self.assertEqual(escapes, [("U1", "channel")])
        self.assertNotEqual(routed, "in.kicad_pcb")
        # The reviewer's end-to-end condition: the board handed to the general
        # router carries the escape copper, not just the intent to escape.
        self.assertEqual(load_adapter()._copper_by_net(output), {"U1_N0": 1})

    def test_an_inferred_package_with_nothing_to_escape_leaves_the_board_as_exported(self):
        # bga_fanout on a SOIC with an exposed pad: ran, requested no ball, wrote
        # the board back with the project's clearances rewritten. Routing that
        # board is routing at 0.127 mm and being judged at the class clearance.
        routed, escapes, _ = run_fanout(
            ["U1"], required=[],
            plan={"requested": 0, "escaped": 0, "failed": 0, "copper": 0})
        self.assertEqual(escapes, [("U1", "channel")])
        self.assertEqual(routed, "in.kicad_pcb")

    def test_via_in_pad_permits_the_under_pad_default(self):
        _, escapes, _ = run_fanout(["U1"], required=[], allow_via_in_pad=True)
        self.assertEqual(escapes, [("U1", "underpad")])

    def test_a_fanout_that_drops_balls_fails_the_route(self):
        # bga_fanout exits 0 after dropping balls it could not escape, so its
        # exit status alone would pass an unescaped package to the router.
        with self.assertRaises(RuntimeError):
            run_fanout(["U1"], required=[],
                       plan={"requested": 2, "escaped": 1, "failed": 1,
                             "unescaped_nets": ["N1"]})

    def test_a_fanout_that_reports_escapes_but_writes_no_copper_fails(self):
        with self.assertRaises(RuntimeError):
            run_fanout(["U1"], required=[], plan={"copper": 0})

    def test_a_fanout_with_no_ledger_fails_the_route(self):
        with self.assertRaises(RuntimeError):
            run_fanout(["U1"], required=[], plan={"summary": False})


class FailClosedPrerequisiteTest(unittest.TestCase):
    def test_missing_route_planes_fails_when_declared(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "krt"
            (repo / "py_router").mkdir(parents=True)
            (repo / "py_router" / "bga_fanout.py").write_text(FANOUT_STUB)
            board = Path(tmp) / "in.kicad_pcb"
            board.write_text('(kicad_pcb (zone (net_name "GND") "F.Cu"))')
            adapter = load_adapter()
            with self.assertRaises(RuntimeError):
                adapter._fanout(
                    sys.executable, repo, board, ["U1:channel:"], "underpad", 0.127,
                    None, None, "standard", False, "none", False, {"U1"},
                )

    def test_failing_route_planes_fails_when_declared(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "krt"
            (repo / "py_router").mkdir(parents=True)
            (repo / "py_router" / "route_planes.py").write_text("import sys; sys.exit(1)")
            (repo / "py_router" / "bga_fanout.py").write_text(FANOUT_STUB)
            board = Path(tmp) / "in.kicad_pcb"
            board.write_text('(kicad_pcb (zone (net_name "GND") "F.Cu"))')
            adapter = load_adapter()
            with self.assertRaises(RuntimeError):
                adapter._fanout(
                    sys.executable, repo, board, ["U1:channel:"], "underpad", 0.127,
                    None, None, "standard", False, "none", False, {"U1"},
                )

    def test_missing_bga_fanout_fails_when_declared(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "krt"
            (repo / "py_router").mkdir(parents=True)
            board = Path(tmp) / "in.kicad_pcb"
            board.write_text("(kicad_pcb)")
            adapter = load_adapter()
            with self.assertRaises(RuntimeError):
                adapter._fanout(
                    sys.executable, repo, board, ["U1:channel:"], "underpad", 0.127,
                    None, None, "standard", False, "none", False, {"U1"},
                )


    def test_missing_bga_fanout_fails_a_grid_array_too(self):
        # Nothing was declared; U1 was selected because it is a grid. Routing
        # it unescaped is the result the stage exists to prevent.
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "krt"
            (repo / "py_router").mkdir(parents=True)
            board = Path(tmp) / "in.kicad_pcb"
            board.write_text("(kicad_pcb)")
            adapter = load_adapter()
            with self.assertRaises(RuntimeError):
                adapter._fanout(
                    sys.executable, repo, board, ["U1:channel:"], "underpad", 0.127,
                    None, None, "standard", False, "none", False, set(),
                )


# Each stage stub logs its argv, then behaves as `<stage>.json` says:
# `{"exit": n, "write": bool}`. Absent, it succeeds and writes its output.
STAGE_STUB = """\
import json, sys
from pathlib import Path

here = Path(__file__).parent
name = Path(__file__).stem
with (here / (name + ".log")).open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
plan_file = here / (name + ".json")
plan = json.loads(plan_file.read_text()) if plan_file.exists() else {}
if plan.get("write", True):
    out = sys.argv[sys.argv.index("--output") + 1] if "--output" in sys.argv else sys.argv[2]
    if name == "bga_fanout":
        # The escape adds a segment per package to the board it was given and
        # prints the ledger bga_fanout prints; the pour and route stages just
        # write a board.
        component = sys.argv[sys.argv.index("--component") + 1]
        names = [f"{component}_N{i}" for i in range(plan.get("copper", 1))]
        copper = "".join(
            f'(segment (start 0 0) (end 1 1) (width 0.2) (layer "F.Cu") '
            f'(net "{net}") (uuid 00000000-0000-0000-0000-{i:012d}))'
            for i, net in enumerate(names)
        )
        source = Path(sys.argv[1]).read_text()
        Path(out).write_text(source.replace("(kicad_pcb)", "(kicad_pcb) " + copper, 1))
        print("JSON_SUMMARY: " + json.dumps({
            "requested": plan.get("requested", 1),
            "escaped": plan.get("escaped", 1),
            "failed": plan.get("failed", 0),
            "unescaped_nets": plan.get("unescaped_nets", []),
        }))
    else:
        Path(out).write_text("(kicad_pcb)")
sys.exit(plan.get("exit", 0))
"""

# The route stage for the end-to-end tests copies the board it was handed, so
# the test can inspect the escape geometry that reached the general router.
ROUTE_COPY_STUB = """\
import json, sys
from pathlib import Path

here = Path(__file__).parent
with (here / "calls.log").open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
Path(sys.argv[sys.argv.index("--json-out") + 1]).write_text(
    json.dumps({"successful": 2, "failed": 0}))
Path(sys.argv[2]).write_text(Path(sys.argv[1]).read_text())
"""

PLANE_BOARD = '(kicad_pcb (zone (net 1) (net_name "GND") (layer "In1.Cu")))'


def run_staged(fanout_components, extra=(), planes=None, fanout=None,
               missing=(), board_text=PLANE_BOARD, stale=()):
    """Run the whole adapter with stub pour, fanout and route stages.

    `planes`/`fanout` configure those stubs; `missing` names stages to leave
    out of the checkout; `stale` names stage outputs a previous run left.
    Returns (exit code, stderr, {stage: [argv lines]}, output board exists)."""
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "krt"
        router = repo / "py_router"
        router.mkdir(parents=True)
        (router / "route.py").write_text(ROUTE_COPY_STUB)
        for stage, plan in (("route_planes", planes), ("bga_fanout", fanout)):
            if stage in missing:
                continue
            (router / f"{stage}.py").write_text(STAGE_STUB)
            if plan is not None:
                (router / f"{stage}.json").write_text(json.dumps(plan))
        board = Path(tmp) / "in.kicad_pcb"
        board.write_text(board_text)
        for name in stale:
            (Path(tmp) / name).write_text("(kicad_pcb)")
        output = Path(tmp) / "out.kicad_pcb"
        command = [
            sys.executable, str(ADAPTER), str(board), str(output),
            "--repo", str(repo), "--python", sys.executable,
            "--fab-tier", "standard",
        ]
        for spec in fanout_components:
            command.extend(("--fanout-component", spec))
        command.extend(extra)
        done = subprocess.run(command, capture_output=True, text=True, check=False)
        logs = {}
        for stage, log in (("planes", "route_planes.log"), ("fanout", "bga_fanout.log"),
                           ("route", "calls.log")):
            path = router / log
            logs[stage] = path.read_text().splitlines() if path.exists() else []
        logs["output"] = output.read_text() if output.exists() else ""
        return done.returncode, done.stderr, logs, output.exists()


class StagedAdapterTest(unittest.TestCase):
    """The adapter end to end: pour, escape, route, against stub stages."""

    FLOORS = ("--escape-clearance", "0.2", "--escape-via-size", "0.6",
              "--escape-via-drill", "0.3", "--allow-via-in-pad")

    def test_the_route_runs_on_the_escaped_board_with_the_floor_applied(self):
        code, _, logs, routed = run_staged(["U1"], extra=self.FLOORS)
        self.assertEqual((code, routed), (0, True))
        # The pour is given the zone's net and its single layer.
        self.assertEqual(len(logs["planes"]), 1)
        self.assertIn("--nets GND --plane-layers In1.Cu", logs["planes"][0])
        # The escape runs on the poured board, at the board's floors.
        self.assertEqual(len(logs["fanout"]), 1)
        escape = logs["fanout"][0]
        self.assertIn("in.poured.kicad_pcb --component U1", escape)
        for expected in ("--escape-method underpad", "--clearance 0.2",
                         "--via-size 0.6 --via-drill 0.3", "--fab-tier standard",
                         "--strict-sizes"):
            self.assertIn(expected, escape)
        # And the general router is handed the escaped board, not the input.
        self.assertEqual(logs["route"][0].split()[0].rsplit("/", 1)[1],
                         "in.fanned0.kicad_pcb")

    def assert_escape_failed(self, code, stderr, logs, routed):
        self.assertEqual(code, 4, stderr)
        self.assertEqual(logs["route"], [], "the general router must not run")
        self.assertFalse(routed, "a failed escape must leave no output board")
        self.assertIn("error: ", stderr)

    def test_a_failed_pour_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              planes={"exit": 1}))

    def test_a_pour_that_writes_nothing_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              planes={"write": False}))

    def test_a_missing_pour_stage_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              missing=("route_planes",)))

    def test_a_failed_grid_array_escape_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              fanout={"exit": 1}))

    def test_a_failed_declared_escape_fails_the_route(self):
        self.assert_escape_failed(*run_staged(
            ["U1:channel:"], extra=("--escape-required", "U1"), fanout={"exit": 1}))

    def test_an_escape_that_writes_nothing_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              fanout={"write": False}))

    def test_a_previous_runs_escape_is_not_taken_for_this_ones(self):
        # The stub exits 0 without writing; a leftover from an earlier run
        # sits where its output would go.
        self.assert_escape_failed(*run_staged(
            ["U1"], extra=self.FLOORS, fanout={"write": False},
            stale=("in.fanned0.kicad_pcb", "in.poured.kicad_pcb")))

    def test_a_missing_escape_stage_fails_the_route(self):
        self.assert_escape_failed(*run_staged(["U1"], extra=self.FLOORS,
                                              missing=("bga_fanout",)))

    def test_a_board_without_planes_escapes_without_pouring(self):
        code, _, logs, routed = run_staged(["U1:channel:"], missing=("route_planes",),
                                           board_text="(kicad_pcb)")
        self.assertEqual((code, routed), (0, True))
        self.assertEqual(len(logs["fanout"]), 1)

    def test_under_pad_without_via_in_pad_falls_back_to_channel_escape(self):
        # The board's own rules decline under-pad escape, so an inferred grid
        # falls back to channel escape instead of routing the unescaped board.
        code, stderr, logs, routed = run_staged(["U1"])
        self.assertEqual((code, routed), (0, True))
        self.assertEqual(len(logs["fanout"]), 1)
        self.assertIn("falling back to channel escape", stderr)
        parts = logs["fanout"][0].split()
        idx = parts.index("--escape-method")
        self.assertEqual(parts[idx + 1], "channel")
        # End to end: the escape geometry reaches the general router's board.
        self.assertEqual(load_adapter()._copper_by_net(logs["output"]), {"U1_N0": 1})

    def test_under_pad_without_via_in_pad_fails_a_declared_escape(self):
        self.assert_escape_failed(*run_staged(["U1:underpad:"],
                                              extra=("--escape-required", "U1")))

    def test_a_stale_output_board_does_not_survive_a_failed_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "krt"
            (repo / "py_router").mkdir(parents=True)
            (repo / "py_router" / "route.py").write_text(STUB)
            board = Path(tmp) / "in.kicad_pcb"
            board.write_text("(kicad_pcb)")
            output = Path(tmp) / "out.kicad_pcb"
            output.write_text("(kicad_pcb) from an earlier run")
            done = subprocess.run(
                [sys.executable, str(ADAPTER), str(board), str(output),
                 "--repo", str(repo), "--python", sys.executable,
                 "--fanout-component", "U1:channel:"],
                capture_output=True, text=True, check=False,
            )
            self.assertEqual(done.returncode, 4, done.stderr)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
