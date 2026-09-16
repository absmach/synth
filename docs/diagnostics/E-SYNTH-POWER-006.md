# E-SYNTH-POWER-006 — decoupling capacitance below required value

**Severity:** warning
**Stage:** erc — power (value-based)

## What this means

The part's manifest declares `required_decoupling` with a value (e.g. `10uF` on `vin`), and capacitors *are* present on that net — so `E-SYNTH-POWER-001` is satisfied — but their summed capacitance is below the required value. A lone `100nF` cap does not replace a required `10µF` bulk capacitor: the rail will droop under load transients.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"   // requires 10uF-class bulk on vin/vout
  component C1: capacitor "c_generic_0805" value "100n"
  connect U1.vin -> C1.p1
  connect U1.gnd -> C1.p2
}
```

## Suggested fix

Raise the bulk capacitance on the net to at least the manifest value (increase the capacitor value or parallel several caps). Nets containing any capacitor with a missing or unparseable `value` are skipped — give every decoupling cap an explicit SI value (`100n`, `10u`) so the rule can weigh it.

## See also

`E-SYNTH-POWER-001` (decoupling count), `E-SYNTH-CRYSTAL-001` (the other value-based rule).
