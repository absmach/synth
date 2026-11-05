# W-SYNTH-IMPEDANCE-001 — impedance not verified on inner layers

**Severity:** warning
**Stage:** erc — rf

## What this means

A single-ended controlled-impedance net, or a differential pair with an `impedance`, is on a board whose `stackup` has inner copper layers. The router may place the net on an inner layer, and Synth derives and checks only outer-layer microstrip width (and, for a pair, gap), so the impedance there is not verified.

The estimate Synth does make (about 10 percent for a single-ended net; not characterised for a pair) does not replace expert analysis.

## Minimal reproduction

```synth
board "x" {
  layers 4
  component U1: antenna "ant_chip_2g4"
  component R1: resistor "r_generic_0603" value "10k"
  connect U1.feed -> R1.p1
  diff_pair u1_feed r1_p1 { impedance 50ohm }
  stackup {
    copper 0.035mm
    insulator 0.2104mm er 4.4
    copper 0.0152mm
    insulator 1.065mm er 4.6
    copper 0.0152mm
    insulator 0.2104mm er 4.4
    copper 0.035mm
  }
}
```

A net is not reported when it is pinned to outer copper: every net of a single-ended class, or both legs of a pair, in a `netclass` whose `layers` are only `F.Cu` and/or `B.Cu`.

## Suggested fix

Pin the net to the outer layer its impedance was derived for, by putting it (or both legs of the pair) in a netclass that declares the layers:

```synth
netclass "ETH" {
  layers "F.Cu"
}
connect U1.txp -> J1.td_p as "ETH_TXP" class "ETH"
connect U1.txn -> J1.td_n as "ETH_TXN" class "ETH"
```

The export writes the pin as a KiCad custom DRC rule, the router keeps the nets on those layers, and routed copper anywhere else is `E-SYNTH-DRC-023`. Otherwise, have the impedance of the routed net reviewed on the layer it uses, or use a two-layer board where every trace is outer-layer microstrip.
