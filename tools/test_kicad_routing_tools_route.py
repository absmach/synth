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


FANOUT_STUB = """\\
import sys
from pathlib import Path

here = Path(__file__).parent
with (here / "fanout.log").open("a") as log:
    log.write(" ".join(sys.argv[1:]) + "\\n")
Path(sys.argv[sys.argv.index("--output") + 1]).write_text("(kicad_pcb)")
"""


def load_adapter():
    import importlib.util

    spec = importlib.util.spec_from_file_location("krt_adapter", ADAPTER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_fanout(components, required, allow_via_in_pad=False):
    """Call `_fanout` against a stub engine; return (board routed next, escapes run)."""
    adapter = load_adapter()
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "krt"
        (repo / "py_router").mkdir(parents=True)
        (repo / "py_router" / "bga_fanout.py").write_text(FANOUT_STUB)
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
        ]


class FanoutTest(unittest.TestCase):
    def test_a_declared_channel_escape_runs_without_via_in_pad(self):
        # U1 takes the under-pad default and is skipped; U2 asked for channel,
        # which puts no via in a pad, so it is escaped and nothing fails.
        routed, escapes = run_fanout(["U1", "U2:channel:"], required=["U2"])
        self.assertEqual(escapes, [("U2", "channel")])
        self.assertNotEqual(routed, "in.kicad_pcb")

    def test_a_declared_underpad_escape_without_via_in_pad_fails_the_route(self):
        with self.assertRaises(RuntimeError):
            run_fanout(["U1:underpad:"], required=["U1"])

    def test_nothing_to_escape_leaves_the_board_untouched(self):
        routed, escapes = run_fanout(["U1"], required=[])
        self.assertEqual((routed, escapes), ("in.kicad_pcb", []))

    def test_via_in_pad_permits_the_under_pad_default(self):
        _, escapes = run_fanout(["U1"], required=[], allow_via_in_pad=True)
        self.assertEqual(escapes, [("U1", "underpad")])


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
STAGE_STUB = """\\
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
    Path(out).write_text("(kicad_pcb)")
sys.exit(plan.get("exit", 0))
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
        (router / "route.py").write_text(STUB)
        (router / "summaries.json").write_text(json.dumps([CLEAN]))
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

    def test_under_pad_without_via_in_pad_skips_a_grid_array(self):
        # The board's own rules decline the escape; nothing failed, so the
        # route runs, on the input board, and says why on stderr.
        code, stderr, logs, routed = run_staged(["U1"])
        self.assertEqual((code, routed), (0, True))
        self.assertEqual((logs["planes"], logs["fanout"]), ([], []))
        self.assertIn("skipping under-pad escape of U1", stderr)
        self.assertEqual(logs["route"][0].split()[0].rsplit("/", 1)[1], "in.kicad_pcb")

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
