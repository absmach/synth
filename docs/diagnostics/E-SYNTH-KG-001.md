# E-SYNTH-KG-001 — support circuit missing per the knowledge graph

**Severity:** from the knowledge template (error or warning)
**Stage:** erc — required support

## What this means

The circuit-design knowledge graph (`crates/synth-knowledge`, seeded
from `knowledge/circuits.toml`) matched one of its enforced templates
against the board and the support circuit it requires is missing.

Templates currently enforced by this code:

| Template             | Fires when                                                                                               | Why production hardware needs it                                                  |
| -------------------- | -------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `switch_debounce_rc` | a mechanical switch drives an IC input with no series/pull resistor + shunt capacitor on the signal node | contact bounce produces phantom edges: double presses, spurious interrupts        |
| `switch_pull_up`     | a switch-to-ground input has no pull-up on its signal node                                               | an open switch leaves the input floating at an undefined level                    |
| `led_current_limit`  | an LED has no series resistor on either terminal                                                         | a forward-biased LED without limiting is a short: overcurrent, drift, burnout     |
| `ic_decoupling`      | an IC (identified by its power pins, any kind) has no decoupling capacitor on a power net                | switching current bursts turn supply inductance into rail droop and ground bounce |

Templates enforced elsewhere (`i2c_pullups` → `E-SYNTH-I2C-001`,
`crystal_load_caps` → layout + `E-SYNTH-CRYSTAL-001`, `usb_esd` →
layout cluster, `reset_rc` → layout + `E-SYNTH-RESET-001`,
`relay_flyback_diode` → activates with the first relay part) never
fire from this code.

## Minimal reproduction

```synth
board "x" {
  component SW1: switch "spst_tactile"
  component U1: mcu "stm32f103c8"
  connect SW1.p1 -> U1.pa0
  connect SW1.p2 -> U1.vss
  // No pull-up / debounce network on the SW1.p1 node — KG-001 fires.
}
```

## Suggested fix

Depends on the template:

- `switch_debounce_rc`: an insertion patch adding a pull-up (or
  pull-down, per the switch's reference rail) resistor and a filter
  capacitor on the signal node, referenced to the peer IC's rails.
  Apply with `synth fix` or `synth_apply_patch`.
- `switch_pull_up`: an insertion patch adding the pull resistor.
- `led_current_limit`: diagnostic only — inserting a series resistor
  requires splitting an existing connection, which pure insertion
  cannot express; rewire in the `.synth` source (e.g.
  `LED — R — GPIO`).
- `ic_decoupling`: add a 100 nF capacitor between the power net and
  ground near the IC (`E-SYNTH-POWER-001` emits an insertion patch
  for parts that declare `required_decoupling`).

## See also

- `docs/knowledge-graph.md` — schema and how to add templates
- `E-SYNTH-POWER-001` — manifest-declared decoupling (the KG skips
  parts that declare it, so nothing is double-flagged)
