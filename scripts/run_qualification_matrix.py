#!/usr/bin/env python3
"""Run the qualification matrix (#45).

Every row of `qualification/matrix.toml` is a versioned reference board with a
declared capability class and the status its release command must produce. This
script runs each row's command — the same `synth check` gate a release uses —
and compares the result against the row's expectations:

- `expect = "pass"`  → source and DRC must both pass, so the run record carries
  a real DRC verdict, never an unavailable check;
- `expect = "fail"`  → the source stage must fail: a negative board is a
  negative *test*, so an infrastructure failure that merely produced a non-zero
  exit is not enough;
- `expect_fab`       → the manufacturing stage must come out exactly as
  declared (`pass`/`fail`). Rows without one are recorded, not asserted.

Each row's record also carries the KiCad version that judged it, the design's
SHA-256, the artifact hashes the manufacturing package reports, and which
native reports were present, so a reviewer can see what was actually checked.
A row whose directory holds a `SHA256SUMS` covering the design source gets that
source verified too, so an accidental fixture edit fails here rather than
quietly changing what a "pass" means.

Results are written as one JSON document (default: `qualification/matrix.json`
under a scratch directory given with `--out`) and the script exits non-zero on
any unexpected verdict.

Usage:
    scripts/run_qualification_matrix.py [--out DIR] [--synth PATH]
                                        [--only ID ...] [--skip ID ...]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # re-exec under a Python that has one (3.11+)
    import shutil

    for _name in ("python3.14", "python3.13", "python3.12", "python3.11"):
        _found = shutil.which(_name)
        if _found:
            os.execv(_found, [_found, __file__, *sys.argv[1:]])
    raise SystemExit("the matrix manifest needs Python 3.11+ (tomllib); none found")

REPO_ROOT = Path(__file__).resolve().parent.parent
MATRIX_PATH = REPO_ROOT / "qualification" / "matrix.toml"


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def kicad_cli_version() -> str:
    """The KiCad version the native checks ran under, or a plain note."""
    try:
        out = subprocess.run(
            ["kicad-cli", "version"], capture_output=True, text=True, timeout=60
        )
        version = out.stdout.strip()
        return version or "unavailable"
    except (OSError, subprocess.TimeoutExpired):
        return "unavailable"


def verify_declared_sources(row: dict) -> dict:
    """Check the row's design source against its committed `SHA256SUMS`.

    A corpus row whose `.synth` hash is recorded in a sibling `SHA256SUMS` must
    still match it: an edited fixture is a changed claim, and the matrix is
    what notices. A row with no such file declares nothing to verify.
    """
    path = REPO_ROOT / row["path"]
    sums = path.parent / "SHA256SUMS"
    if not sums.is_file():
        return {"verified": None, "reason": "no SHA256SUMS beside the design"}
    design_name = path.name
    recorded = {}
    for line in sums.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        digest, _, name = line.partition("  ")
        recorded[name.strip()] = digest
    if design_name not in recorded:
        return {
            "verified": None,
            "reason": f"{design_name} is not recorded in SHA256SUMS",
        }
    actual = sha256_of(path)
    if actual != recorded[design_name]:
        return {
            "verified": False,
            "reason": f"{design_name} no longer matches its committed SHA-256",
        }
    return {"verified": True, "reason": f"{design_name} matches its committed SHA-256"}


def run_row(
    row: dict, synth: Path, out_dir: Path, qualify_router: str, stage_budget_secs: int
) -> dict:
    """Run one row and compare what came back against what it declares."""
    board = REPO_ROOT / row["path"]
    flags = row.get("flags", [])
    # A row can name the engine each command records: the corpus records which
    # router produced its evidence, so the matrix re-runs it with that engine
    # pinned (issue #95's pin semantics: a pin never falls back).
    router = row.get("router", qualify_router)
    environment = dict(os.environ)
    environment["SYNTH_QUALIFY_ROUTER"] = router
    # Real routes take minutes, so the matrix raises the check gate's own
    # per-stage child budget; the router's wall clock never runs shorter than
    # that stage budget.
    environment["SYNTH_CHECK_STAGE_BUDGET_SECS"] = str(stage_budget_secs)
    existing_router_budget = environment.get("SYNTH_ROUTER_TIMEOUT_SECS") or "0"
    try:
        router_budget = int(existing_router_budget)
    except ValueError:
        router_budget = 0
    environment["SYNTH_ROUTER_TIMEOUT_SECS"] = str(max(stage_budget_secs, router_budget))
    # Parts the shipped registry does not carry live beside the board.
    if row.get("user_registry"):
        environment["SYNTH_USER_REGISTRY_DIR"] = str(
            (REPO_ROOT / row["path"]).parent / row["user_registry"]
        )
    command = [str(synth), "check", str(board), "--json", *flags]

    def invoke(args: list[str], timeout: int, env: dict[str, str]) -> dict:
        try:
            done = subprocess.run(
                args, capture_output=True, text=True, timeout=timeout, env=env
            )
            return {"exit": done.returncode, "stdout": done.stdout, "stderr": done.stderr}
        except subprocess.TimeoutExpired as error:
            return {
                "exit": None,
                "stdout": error.stdout or "",
                "stderr": (error.stderr or "") + f"\ncommand timed out after {timeout}s",
            }

    started = invoke(command, 60 * 60, environment)
    try:
        report = json.loads(started["stdout"]) if started["stdout"].strip() else {}
    except json.JSONDecodeError:
        report = {}
    stages = report.get("stages", {})

    record = {
        "id": row["id"],
        "class": row.get("class"),
        "declared_status": row.get("status"),
        "path": row["path"],
        "router": router,
        "fab_router": row.get("fab_router", row.get("router", qualify_router))
        if "expect_fab" in row
        else None,
        "command": command,
        "exit": started["exit"],
        "source_status": stages.get("source", {}).get("status"),
        "drc_status": stages.get("drc", {}).get("status"),
        "source_sha256": report.get("source_sha256"),
        "drc_counts": (stages.get("drc", {}).get("result") or {}).get("validation", {}).get(
            "kicad_drc"
        ),
        "artifacts": stages.get("manufacturing", {}).get("artifacts", {}),
        "native_stages": [
            {"stage": stage.get("stage"), "status": stage.get("status")}
            for stage in (stages.get("manufacturing", {}).get("native") or {}).get("stages", [])
        ],
        "declared_sources": verify_declared_sources(row),
        "kicad_cli_version": kicad_cli_version(),
    }

    # The manufacturing package is only run for rows that declare an
    # expectation for it, so an unbounded export never runs uninvited. A row
    # can name a different engine for it (`fab_router`): the corpus records
    # the router whose copper the fab verdict judged.
    if "expect_fab" in row:
        fab_router = row.get("fab_router", router)
        fab_environment = dict(environment)
        fab_environment["SYNTH_QUALIFY_ROUTER"] = fab_router
        fab_argv = [str(synth), "check", str(board), "--fab", "--json", *flags]
        fab = invoke(fab_argv, 90 * 60, fab_environment)
        try:
            fab_report = json.loads(fab["stdout"]) if fab["stdout"].strip() else {}
        except json.JSONDecodeError:
            fab_report = {}
        fab_stages = fab_report.get("stages", {})
        record["manufacturing_status"] = fab_stages.get("manufacturing", {}).get("status")
        record["fab_exit"] = fab["exit"]
        record["artifacts"] = (
            fab_stages.get("manufacturing", {}).get("artifacts") or record["artifacts"]
        )
        record["native_stages"] = [
            {"stage": stage.get("stage"), "status": stage.get("status")}
            for stage in (fab_stages.get("manufacturing", {}).get("native") or {}).get(
                "stages", []
            )
        ]

    record["verdict"] = judge(row, record)
    (out_dir / f"{row['id']}.json").write_text(json.dumps(record, indent=2, sort_keys=True))
    return record


def judge(row: dict, record: dict) -> str:
    """`pass` when the row did what it declares, `fail` with reasons otherwise."""
    reasons = []
    expect = row.get("expect")
    source_ok = record["source_status"] == "pass"
    drc_ok = record["drc_status"] == "pass"
    if expect == "pass":
        if not source_ok:
            reasons.append(f"source stage is {record['source_status']}, expected pass")
        if not drc_ok:
            reasons.append(f"drc stage is {record['drc_status']}, expected pass")
    elif expect == "fail":
        if record["source_status"] != "fail":
            reasons.append(
                f"source stage is {record['source_status']}, expected fail: a negative "
                "row fails at the source gate, not by accident"
            )
    if "expect_fab" in row:
        expected = row["expect_fab"]
        manufacturing = record.get("manufacturing_status")
        if expected and manufacturing != expected:
            reasons.append(f"manufacturing stage is {manufacturing}, expected {expected}")
    if record["declared_sources"]["verified"] is False:
        reasons.append(f"declared sources drifted: {record['declared_sources']['reason']}")
    if record["exit"] is None:
        reasons.append("the release command timed out")
    return "pass" if not reasons else "fail: " + "; ".join(reasons)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=None, help="results directory")
    parser.add_argument("--synth", type=Path, default=None, help="path to the synth binary")
    parser.add_argument("--only", nargs="*", default=None, help="run only these row ids")
    parser.add_argument("--skip", nargs="*", default=[], help="skip these row ids")
    parser.add_argument(
        "--qualify-router",
        default="kicad-routing-tools",
        help="value recorded in SYNTH_QUALIFY_ROUTER for every row",
    )
    parser.add_argument(
        "--stage-budget-secs",
        type=int,
        default=1800,
        help="SYNTH_CHECK_STAGE_BUDGET_SECS for every row: real routes take minutes",
    )
    args = parser.parse_args()

    matrix = tomllib.loads(MATRIX_PATH.read_text())
    boards = matrix.get("board", [])
    out_dir = args.out or Path(tempfile.mkdtemp(prefix="synth-matrix-"))
    out_dir.mkdir(parents=True, exist_ok=True)

    synth = args.synth
    if synth is None:
        candidate = REPO_ROOT / "target" / "debug" / "synth"
        synth = candidate if candidate.is_file() else Path("synth")

    selected = [
        row
        for row in boards
        if (args.only is None or row["id"] in args.only) and row["id"] not in args.skip
    ]
    # A row the corpus cannot run on this code declares why (`skip_reason`):
    # it is recorded, never silently dropped, and never counted as a matrix
    # failure. What cannot run is part of what the matrix reports (issue #45).
    skipped = [
        {"id": row["id"], "path": row["path"], "reason": row["skip_reason"]}
        for row in selected
        if row.get("skip_reason")
    ]
    runnable = [row for row in selected if not row.get("skip_reason")]
    records = [
        run_row(row, synth, out_dir, args.qualify_router, args.stage_budget_secs)
        for row in runnable
    ]

    results = {
        "schema_version": "synth.qualification.v1",
        "release_command": matrix["matrix"]["release_command"],
        "fab_command": matrix["matrix"]["fab_command"],
        "qualify_router": args.qualify_router,
        "stage_budget_secs": args.stage_budget_secs,
        "kicad_cli_version": records[0]["kicad_cli_version"] if records else kicad_cli_version(),
        "skipped_rows": skipped,
        "rows": records,
    }
    (out_dir / "results.json").write_text(json.dumps(results, indent=2, sort_keys=True))

    failed = [record for record in records if not record["verdict"].startswith("pass")]
    for record in skipped:
        print(f"{record['id']}: cannot run ({record['reason']})")
    for record in records:
        print(f"{record['id']}: {record['verdict']}")
    print(f"results: {out_dir / 'results.json'}")
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
