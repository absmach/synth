# E-SYNTH-PARSE-033 — invalid `legends`/`labels` value

**Severity:** error
**Stage:** parse

## What this means

A `legends` or `labels` board statement names something other than `on` or
`off`. Both statements are switches spelled exactly `on` or `off`.

## Minimal reproduction

```synth
board "x" {
  legends maybe
}
```

## Suggested fix

Write `legends on` to emit compact connector pin legends, or `legends off`
(the default) to leave them out. Write `labels off` to suppress the
exporter-added reference and value silkscreen/fab text on the exported PCB,
or `labels on` (the default) to keep them. The diagnostic carries a patch
replacing the word with the statement's default (`off` for `legends`, `on`
for `labels`).
