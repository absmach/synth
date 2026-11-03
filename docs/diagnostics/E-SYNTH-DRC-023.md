# E-SYNTH-DRC-023 — net routed off its netclass layers

**Severity:** error (blocking)
**Stage:** drc — signal integrity

## What this means

A net belongs to a `netclass` that declares `layers`, and the routed copper puts some of the net's track on a layer the class does not allow.

The export writes the pin as a KiCad custom DRC rule and the router is asked to keep the class on its layers, but neither of those is evidence that it did. This check reads the routed copper. The case it exists for is a controlled-impedance pair pinned to the outer layer its width and gap were derived for: routed on an inner layer it is stripline, the derived geometry no longer gives the target impedance, and nothing else would say so.

A via is not judged by itself. A through via spans every layer, and a net routed only on its allowed layers uses one only to change between them, which its tracks already show.

## Minimal reproduction

```synth
board "b" {
  layers 4
  netclass "ETH" {
    layers "F.Cu"
  }
  component R1: resistor "r_generic_0603" value "49.9"
  component R2: resistor "r_generic_0603" value "49.9"
  connect R1.p1 -> R2.p1 as "ETH_TXP" class "ETH"
}
```

Routed with `ETH_TXP` dropping to `In1.Cu` for part of its length, the net is reported.

## Suggested fix

Re-route the net on the allowed layers, or widen `layers` if the class may genuinely use another one. Widening it to an inner layer on a controlled-impedance class brings back `W-SYNTH-IMPEDANCE-001`, because only outer-layer microstrip is derived.
