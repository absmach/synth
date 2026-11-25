# Reference subcircuits

Vetted circuits copied 1:1 from a data sheet, application note or
specification. A design that uses one gets checked source code instead of a
circuit drawn from memory. Every departure from the reference is a written
exception that `synth validate` lists in one place.

| Module | Copies |
|--------|--------|
| `usb_c_sink_esd` | USB Type-C R2.1 Rd sink, PRTR5V0U2X D+/D- clamp |
| `ldo_ams1117_3v3` | AMS1117-3.3 typical application |
| `buck_lmr33640_5v` | LMR33640 data sheet Table 9-2, 5 V at 400 kHz |
| `crystal_load_caps` | ST AN2867 Pierce load capacitors |
| `i2c_pullups` | NXP UM10204 pull-up sizing |

## Layout

```text
<id>/
  module.synth   the `module "Name" (…) { … }` declaration
  module.toml    id, version, provenance, ports, params, validated values, notes
  fixture.synth  a board that uses the module and must validate clean
```

`module.toml` fields:

- `provenance`: `source` (the document), `section` (the table, figure or
  clause), `reference_design` and `url`.
- `[[params]]`: `name`, `default`, `fixed`, an optional `range`, and a
  `description`. `fixed` must agree with the `fixed` keyword in
  `module.synth`. The loader refuses a mismatch.
- `[[validated]]`: the values a fixture checks.

`crates/synth-validate/tests/reference_modules.rs` validates every fixture.
A module whose fixture has a blocking diagnostic fails the build.

## Using one

MCP: `synth_search_modules`, `synth_get_module`, then
`synth_instantiate_module` with a port-to-net map.

CLI:

```sh
synth module list ldo
synth module show ldo_ams1117_3v3
synth module instantiate ldo_ams1117_3v3 board.synth --label LDO1 \
  --bind vin=VIN --bind vout=+3V3 --bind gnd=GND
```

Both write the module declaration into the design once, with its id and
version in a comment, and add a `use` block.

## Deviations

A parameter marked `fixed` is part of the reference. Overriding it needs a
reason:

```synth
use "LdoAms1117" as LDO1 (c_out = 47uF deviation "polymer part with the ESR the data sheet asks for") { … }
```

With the CLI, pass `--param c_out=47uF --deviation "c_out=polymer part"`. A
bare `--deviation "reason"`, or `deviation "reason"` in the `use`, records
an exception for the instance as a whole.

Without a reason the override is `E-SYNTH-MODULE-008`. Every recorded
deviation appears in `E-SYNTH-MODULE-009` and in the review packet's
`module_deviations`.

## Adding a module

Copy the reference exactly, cite the section, and mark every value the
reference depends on `fixed`. Use only registry parts whose pins have been
checked against their data sheet. Add a fixture that validates with no
blocking diagnostic.
