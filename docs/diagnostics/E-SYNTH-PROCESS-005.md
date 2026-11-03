# E-SYNTH-PROCESS-005 — escape layer is not on this board

**Severity:** error
**Stage:** lowering

## What this means

A component's `escape { layers … }` names copper the board does not have. A
four-layer board has `F.Cu`, `In1.Cu`, `In2.Cu` and `B.Cu`; a two-layer board
has only the outer pair.

An error rather than a name passed through, for the same reason an unknown
`method` is an error. Left alone the name reaches the escape engine and the
fanout fails at route time, which fails the route — after the board has been
exported and the router started, and as a failed escape stage rather than as
the typo it is.

The check runs after the whole board is read, not where the block appears, so
`layers` may be declared anywhere in the file.

## Minimal reproduction

```synth
board "b" {
  layers 2
  component U1: fpga "ice40hx4k_bg121" {
    escape { method underpad  layers "In1.Cu" }
  }
}
```

`In1.Cu` exists only from four layers up.

## Suggested fix

Name a layer the board has, or raise the board's layer count. Dropping the
`layers` list entirely is also valid: the escape then uses whatever copper the
engine distributes across.
