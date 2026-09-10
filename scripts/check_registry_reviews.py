#!/usr/bin/env python3
"""Registry review gate (Phase 15, R15.10).

Fails when a Tier-1 registry change (`registry/parts/**`) lands without a
reviewer: every added or modified `*.synth.toml` must carry a non-empty
`reviewed_by` in its `[provenance]` section. This is the enforcement half
of the risk-register mitigation "registry entries signed": release
signing (`synth registry manifest`) covers integrity, this script covers
authorship.

Usage:
    check_registry_reviews.py <base-ref> [head-ref]

    base-ref   git ref to diff against (e.g. origin/main).
    head-ref   optional; defaults to the current working tree.

Exit codes: 0 = clean, 1 = at least one unreviewed change, 2 = usage error.
"""

import re
import subprocess
import sys

REGISTRY_PREFIX = "registry/parts/"
# A `reviewed_by = "..."` with at least one non-whitespace character inside
# the quotes. TOML basic and literal string forms are both accepted.
REVIEWED_BY_RE = re.compile(r'^\s*reviewed_by\s*=\s*["\'](.+)["\']\s*$', re.MULTILINE)
PROVENANCE_RE = re.compile(r"^\s*\[\s*provenance\s*\]\s*$", re.MULTILINE)


def changed_part_files(base: str, head: str | None) -> list[str]:
    """Return changed Tier-1 part files (added or modified)."""
    diff_filter = "--diff-filter=AM"
    args = ["git", "diff", "--name-only", diff_filter]
    if head:
        args += [base, head]
    else:
        args += [base]
    args += ["--", REGISTRY_PREFIX]
    out = subprocess.run(args, capture_output=True, text=True, check=True)
    return sorted(
        p.strip() for p in out.stdout.splitlines() if p.strip().endswith(".synth.toml")
    )


def reviewed_by_set(text: str) -> bool:
    """True when the file carries a non-empty reviewed_by."""
    return REVIEWED_BY_RE.search(text) is not None


def main() -> int:
    if len(sys.argv) < 2 or len(sys.argv) > 3:
        print(__doc__, file=sys.stderr)
        return 2
    base = sys.argv[1]
    head = sys.argv[2] if len(sys.argv) == 3 else None

    files = changed_part_files(base, head)
    if not files:
        print("registry-review: no Tier-1 part changes; gate passes")
        return 0

    failures = []
    for path in files:
        try:
            with open(path, encoding="utf-8") as f:
                text = f.read()
        except OSError as e:
            failures.append(f"{path}: unreadable ({e})")
            continue
        if not PROVENANCE_RE.search(text):
            failures.append(
                f"{path}: no [provenance] section — Tier-1 changes must name a reviewer"
            )
        elif not reviewed_by_set(text):
            failures.append(
                f"{path}: [provenance].reviewed_by is empty — Tier-1 changes must be reviewed"
            )

    if failures:
        print("registry-review: FAILED — unreviewed Tier-1 registry changes:", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        print(
            "\nSet [provenance].reviewed_by (and reviewed_at) after human review of the "
            "pinout/footprint, or move the part to the Tier-2 user registry.",
            file=sys.stderr,
        )
        return 1

    print(f"registry-review: OK — {len(files)} reviewed change(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
