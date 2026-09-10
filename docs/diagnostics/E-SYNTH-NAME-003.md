# E-SYNTH-NAME-003 — refdes does not start with a letter

**Severity:** warning
**Stage:** erc — naming

## What this means

Standard EDA convention is `<LetterPrefix><Number>` (U1, R3, J5). A refdes that starts with a digit or symbol will not sort cleanly in BoMs and may confuse downstream tools.

## Minimal reproduction

(intentionally omitted — the V1 parser only accepts idents that begin with a letter)

## Suggested fix

Use a conventional EDA-style prefix.
