# Broken MCU board

A board that is meant to fail. It exists to show what Synth says when a design
is wrong, which is only useful if the diagnostics name what is wrong and where.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Layers | 2 |
| Components | 6 |
| Source | `negative_broken_mcu_board.synth` |
| `synth validate` | **exits 1** |

This board is never routed and never exported. Reaching the router would mean
the compiler had let it through.

## The four defects, and what each produces

| Defect | Diagnostic | Count |
| --- | --- | --- |
| Required pins left unconnected | `E-SYNTH-CONNECT-001` | 2 |
| Duplicate reference designators | `E-SYNTH-NAME-001` | 2 |
| A generic passive with no value | `E-SYNTH-VALUE-001` | 1 |

Five error-level diagnostics in total. Each names the component and the pin or
designator at fault, and each points at a line in the source.

`crates/synth-cli/tests/class_a_negative_board.rs` compares that set against
`synth validate --format json` in both directions, so a finding cannot vanish
quietly and a sixth cannot arrive undeclared.

## Why this shape

A negative case is worth having only if it fails the way a real mistake fails.
Three things would make it worthless:

- **Failing at parse.** A syntax error proves the parser works, not the rule
  engine. This board parses cleanly and lowers to IR; every finding is
  semantic.
- **Failing with one diagnostic.** A design can be wrong in several
  independent ways at once, and a compiler that reports the first and stops
  makes a reviewer iterate. All five are reported in one run.
- **Failing silently.** The exit code is 1 and the findings are error-level,
  so a script gating on this board sees the failure.

## Limitations

- The fourth test case asks for an impossible footprint as well. That defect
  is not on this board: a part naming a footprint KiCad does not ship is
  caught at export rather than validation, and this board never reaches
  export. The check exists — `E-SYNTH-EXPORT-001` — and several shipped parts
  trip it, but it is not exercised here.
- The six warnings this board also reports are not part of the exercise, and
  nothing asserts them. A new warning is free to appear.
