# E-SYNTH-NETCLASS-001 — netclass layer is not on this board

**Severity:** error
**Stage:** lowering

## What this means

A `netclass { layers … }` names copper the board does not have. A four-layer
board has `F.Cu`, `In1.Cu`, `In2.Cu` and `B.Cu`; a two-layer board has only
the outer pair.

An error rather than a name passed through: the layers are where the class's
nets may be routed, so a name the board lacks would reach the router and the
exported DRC rule as a layer nothing can be routed on, and every net in the
class would come back unrouted with nothing pointing at the typo.

The check runs after the whole board is read, so `layers 4` may be declared
anywhere in the file.

## Minimal reproduction

```synth
board "b" {
  layers 2
  netclass "ETH" {
    layers "In1.Cu"
  }
}
```

`In1.Cu` exists only from four layers up.

## Suggested fix

Name a layer the board has, or raise the board's layer count.
