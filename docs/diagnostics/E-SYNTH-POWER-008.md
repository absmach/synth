# E-SYNTH-POWER-008 — pull-up rail exceeds the bus device's supply

**Severity:** error
**Stage:** erc — power

## What this means

A bus is pulled up to a rail higher than the supply of a device on that
bus. The usual case is a 5 V pull-up on a 3.3 V I²C bus: the device's
input protection conducts, the bus never reaches a clean high, and the
part can be damaged. `E-SYNTH-POWER-005` catches a rail above a pin's
*absolute maximum*; this rule catches the subtler nominal mismatch.

## Minimal reproduction

```synth
board "x" {
  component U1: regulator "ams1117_3v3"
  component U2: regulator "ams1117_5v"
  component U3: sensor "bmp280_pressure"    // 3.3V device
  component R1: resistor "r_generic_0603" value "4.7k"
  connect U1.vout -> U3.vdd
  connect U3.sda -> R1.p1
  connect R1.p2 -> U2.vout                  // pulled to 5V
}
```

## What this does not report

A **resistor divider** is not a pull-up. When the net is also tied to
ground through a resistor, the pin sees the divided voltage, and that is
what this rule compares against the device's supply — so reading a 12 V
input on a 3.3 V ADC pin through a 100k/10k divider is clean, because
1.09 V reaches the pin.

The ratio is computed rather than waved through: a 1k/100k "divider"
from 5 V still delivers 4.95 V and is still reported.

The exemption needs **exactly one** readable resistor to a rail and
**exactly one** to ground. A second resistor to either side, a branch
off to a third net, or a value the parser cannot read all leave the rule
on the conservative rail voltage. Two resistors to the rail and one to
ground is not a circuit whose output follows from two values, and
treating it as one would suppress a real overvoltage rather than report
it — a check must not clear a violation it cannot compute.

A pin that declares `voltage_max_v` at or above the voltage that
reaches it is exempt too, because a pin that states what it tolerates is
the authority on what it tolerates. The comparison is against the
divided voltage where there is a divider, not the rail: a 12 V rail
divided to 4 V on an input rated 4.5 V is inside its rating. A high-side current monitor runs from 3.3 V and specifies
26 V common mode on its sense inputs; a 5 V-tolerant input on a 3.3 V
part is the same shape. A pin that says nothing still falls back to its
device's supply, so this is a statement by the part rather than a hole
in the rule.

An adjustable regulator's feedback pin is exempt for the same reason,
by its `feedback` capability: the divider from the output rail down to
`fb` is how the part is told what to produce.

## Suggested fix

Pull the bus up to the device's own rail (`U1.vout`), divide it down, or
add a level shifter. Tune the tolerance with `pullup_margin_v` in
`<design>.synth.erc.toml`.
