# Multi-unit symbols and pin functions

Phase 5. Two related jobs: draw a multi-unit package as the several
symbols KiCad expects, and show the peripheral function a pin is used
for rather than its package position.

## Multi-unit symbols

### The model

A registry part may tag its pins with a `unit` (`"A"`, `"B"`, …); a
pin without one is shared by the whole package (the LM358's `vcc_pos` /
`vcc_neg`). **One SynthSpec `component` is one physical package**: the
design refers to `U1.out_a` and `U1.out_b` on a single `U1`, and the
PCB carries one footprint. That is already how `E-SYNTH-NAME-010`
reads units (it groups a component's power pins by `unit`), so the
export side is what had to catch up.

### What was wrong

`build_library` embeds the **stock KiCad symbol** verbatim, and for a
multi-unit part that symbol declares several units
(`LM358_1_1`, `LM358_2_1`, `LM358_3_1`). But the placed instance always
said `(unit 1)`, so KiCad drew unit 1 only and reported:

```
[missing_unit]: Symbol U1 has unplaced units [ B, C ]
[missing_power_pin]: Symbol U1 has input power pins in units [ C ] that are not placed
```

Units 2 and 3 were never placed, so their pins never reached the
netlist — the export was silently incomplete.

### The fix

1. **Loader** (`synth-layout::kicad_lib_loader`): `symbol_units(lib_id)`
   parses the `<Symbol>_<unit>_<style>` sub-symbols (following
   `(extends …)`) into a *pin number → unit* map plus a unit count.
   Unit `0` (common pins) is attributed to unit 1, where the router
   draws it.
2. **Layout**: a multi-unit package keeps **one placement**, and the
   units are drawn as a vertical stack anchored there
   (`UNIT_PITCH_MM = 12.7`). Both pin-position paths — the router's
   `compute_anchor_pin_offset` and the label/router helper
   `pin_terminal_xy` — add `pin_unit_offset(lib_id, number)`, and
   `body_size_for_part` reserves the taller stacked box so the placer
   leaves room.
3. **Exporter**: `build_symbol_instance` returns **one placed symbol
   per unit**, each with its own `(unit N)` and stacked by the same
   offset. Only that unit's pins are listed on it.

Verified end to end: a two-amplifier LM358 design exports with units 1,
2 and 3 placed, `kicad-cli sch erc` no longer reports `missing_unit`,
and every op-amp pin (including the unit-3 power pins) connects.

### Scope

Stock symbols are the supported path; a synthesized (no `kicad_symbol`)
part stays single-unit, which is correct for it. A stock symbol whose
units overlap in a way the vertical stack cannot separate (rare) would
need per-unit geometry from the symbol — not done here.

## Pin functions via alternates

### The model

A pin is named for its package position (`PB6`, `GP0`); a schematic
reads far better when it shows the function in use (`I2C1_SCL`). KiCad
models this with **pin alternates**: the library symbol declares the
possible function names on a pin and the placed instance selects one.

The function in use is derived from the **net name** connected to the
pin, using the same vocabulary as the pin-mux rules
(`synth_registry::PinCapability::from_net_name`), and applied only when
the pin's registry capabilities actually include that function. So
`I2C1_SCL` on a pin that lists `i2c_scl` becomes the pin's alternate;
a net that names no function, or one the pin cannot carry, leaves the
package name in place.

### How it is emitted

Two halves that must agree, both from
`synth-kicad::alternates`:

- `part_pin_alternates(board)` is the union of every function name used
  on a part's pins anywhere in the design. `build_library` **injects**
  those names into the embedded stock symbol
  (`inject_alternates`, a targeted insertion after each pin's
  `(number …)` block) or declares them on the synthesized symbol. A
  symbol that needs no new alternate keeps byte-identical text.
- `pin_function_alternates(board, component)` is the per-instance
  selection, emitted as `(pin "N" (alternate "NAME") (uuid …))`. Its
  names are a subset of the library union by construction.

Verified: a design routing `I2C1_SCL`/`I2C1_SDA` to an RP2350 (whose
stock symbol declares no alternates of its own) exports, and KiCad's ERC
report lists the pin as `[I2C1_SCL, Bidirectional, Line]`.

### Scope

Alternates carry the **net's** function name when it is a legal KiCad
identifier, else the canonical capability name (`I2C_SDA` for a bus
member `I2C0.sda`). Only pins whose net names a function the pin
supports are touched.

## The pin-mux checks

- **`E-SYNTH-PINMUX-001`** (error, lowering): one pin asked to carry
  two functions — two function nets shorted onto the same pin. It is
  reported where both names are still visible, and
  `E-SYNTH-NAME-005` keeps the non-function case so exactly one fires.
- **`E-SYNTH-PINMUX-002`** (error, ERC): a function named by a net
  routed to a pin that does not declare it. This is the name-driven
  complement to the capability-consistency rules, which need a
  dedicated peripheral pin to fire.

See `docs/diagnostics/E-SYNTH-PINMUX-001.md` and `-002.md`.

## Tests

- `synth-layout::kicad_lib_loader` unit tests: unit parsing (synthetic
  dual op-amp, single-unit symbol, and the real `Amplifier_Operational:LM358`
  when KiCad is installed).
- `synth-kicad::alternates` unit tests: name selection, injection
  (idempotent, byte-identical when untouched).
- `synth-kicad::schematic` tests: alternates declared *and* selected;
  a multi-unit part emits `(unit 1)`, `(unit 2)`, `(unit 3)`.
- `synth-validate::deep_erc` tests: both pin-mux rules, including the
  no-double-report and passive-skip cases.
