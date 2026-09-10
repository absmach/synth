# E-SYNTH-NAME-002 — empty refdes

**Severity:** error
**Stage:** erc — naming

## What this means

A component declaration left the reference designator empty. Empty refdes will fail downstream tools and is almost always a typo.

## Minimal reproduction

(intentionally omitted — the parser rejects empty idents at the syntactic level)

## Suggested fix

Give the component a non-empty refdes such as `U1`, `R3`, or `J5`.
