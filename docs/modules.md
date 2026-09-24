# Reusable blocks: modules, interfaces, and buses

## Decision log

Two questions had to be settled before implementing modules, because
they constrain the whole design.

### What does KiCad actually support?

KiCad 10 (`SEXPR_SCHEMATIC_FILE_VERSION 20251012`, "Flat schematic
hierarchy support") documents **three** multi-sheet modes:

| Mode | What it is |
| --- | --- |
| **Flat** | Several top-level sheets, *no* master/root diagram. Sheets are peers. |
| **Simple hierarchy** | A root sheet with sub-sheets; each sub-sheet file is used **once**. |
| **Complex hierarchy** | A sub-sheet **file** is instantiated **several times**; per-instance data lives in each symbol's `(instances (project (path … (reference …))))` block. |

Two constraints follow, and they decide the design:

1. In a **flat** schematic every sheet must have a *different filename*.
   Pointing two sheets at the same file is only legal in a
   **hierarchical** schematic (KiCad warns and loses symbol↔footprint
   links otherwise).
2. Flat sheets connect through **global labels**; local labels are
   strictly sheet-local (briefly not, in 10.0.0–10.0.2; reverted).

So "reuse one sheet file for several instances" is *only* available via
complex hierarchy. Flat hierarchy cannot do it, by construction.

### Decision

**Modules are a source-level reuse construct that flattens to distinct
physical parts.** Instantiating a module twice yields two sets of
components with prefixed reference designators (`CH1_U1`, `CH2_U1`) and
distinct nets. This is the physically correct model — two sensor
channels *are* two sensors — and it keeps the IR flat, which is what
every downstream stage (ERC, power domains, placement, routing, DRC,
BOM) already assumes.

Each instantiation becomes **its own sheet group**, named after the
instance (`component.sheet = "CH1"`). The export is therefore valid in
both flat and hierarchical modes, and never aliases two sheets to one
file. P26 splits a sheet group into its own file when the page
overflows — so a small design keeps every instance on one page (still
flattened, still valid), and a large one gets one file per instance.
Cross-sheet nets use the labels P26 already emits.

Complex hierarchy (one shared sheet file, N instance paths) is
**deliberately not used**, because:

- it requires one shared layout with per-instance reference rewriting,
  while our placer produces per-sheet geometry (`layout_sheets`);
- it buys only schematic-page economy — the electrical result is
  identical to flattening;
- a shared file makes each artifact harder to read and review in
  isolation, and instance-path errors are a known KiCad footgun.

Instance identity is carried by the sheet name plus the refdes prefix
(`CH1_U1`); no extra `module`/`instance` field is threaded through the
IR, because the sheet already gives placement, P26, and any future
complex-hierarchy exporter what they need.

### Consequence for P26

P26 is unchanged and compatible: it already splits on `sheet`
boundaries and emits one file per boundary with distinct filenames.
Module instances simply *are* such boundaries — `use "X" as CH1`
assigns `sheet = "CH1"` to everything it expands to, so P26's existing
machinery splits per instance with no special casing. This is the whole
reason the decision had to be made first: had P26 assumed one file could
host several instances, module instantiation would have had to wait.

## Syntax

```synth
// A reusable, parameterised block. Ports are typed; the types name
// interface bundles or plain directions.
module "SensorChannel" (vdd: power, gnd: ground, i2c: I2C, alert: output) {
  param r_pull: resistance = 4.7kohm
  component U1: sensor "bmp280_pressure"
  component C1: capacitor "c_generic_0603"
  component R1: resistor "r_generic_0603" value $r_pull

  connect U1.vdd -> vdd
  connect U1.gnd -> gnd
  connect U1.sda -> i2c.sda
  connect U1.scl -> i2c.scl
  connect U1.int -> alert
  connect U1.vdd -> C1.p1
  connect C1.p2 -> gnd
  connect U1.sda -> R1.p1
  connect R1.p2 -> vdd
}

// An interface bundle type: named members, bound as a unit.
interface "I2C" (sda: i2c_sda, scl: i2c_scl)

// A bus: a named group of nets, exported as a KiCad bus + bus alias.
bus "I2C0" (sda, scl)
bus "SPI0" (sck, mosi, miso, cs)
bus "USB0" (dp, dn, vbus, gnd)

board "hub" {
  // Instantiated with a refdes prefix; ports bound in one block.
  use "SensorChannel" as CH1 (prefix "CH1_") {
    vdd -> "3V3"
    gnd -> "GND"
    i2c -> "I2C0"          // one binding for the whole bundle
    alert -> "ALERT_1"
  }

  use "SensorChannel" as CH2 (prefix "CH2_", r_pull = 10kohm) {
    vdd -> "3V3"
    gnd -> "GND"
    i2c -> "I2C0"          // second channel, same bus
    alert -> "ALERT_2"
  }
}
```

- A port reference in a body is a bare identifier (`-> vdd`); a quoted
  string (`-> "3V3"`) names a net explicitly.
- `prefix` defaults to `<label>_`, so refdes stay unique without
  spelling it out.
- `i2c -> "I2C0"` binds every member of the `I2C` bundle by name to
  `I2C0.<member>`; members may also be bound individually
  (`i2c.sda -> "I2C0.sda"`).
- `$param` in a component `value` substitutes the instance's parameter.
- `bind "I2C0" : I2C { sda -> U9.gp0 }` ties a declared bus to a part's
  pins: each member lowers to a connection from the `I2C0.<member>` net
  to the target pin.

## How it lowers

Expansion happens once, in `synth-ir/src/modules.rs`, before the normal
statement walk, so every later stage sees a flat board:

- A `use` is replaced by a sheet named after the instance, holding the
  module body with every refdes and internal net prefixed
  (`CH1_U1`, `CH1_<net>`) and every `$param` substituted.
- A port reference resolves to its binding. A whole-interface binding
  expands to one binding per member, so member resolution is uniform.
- A bound net is expressed as a `"NAME"` endpoint. Lowering treats that
  as a **name anchor**, not a pin: two instances binding the same net
  merge into one net by name (the same mechanism `as "NAME"` uses).
- `bus "N" (…)` is **metadata only** — it does not materialize empty
  nets (that would trip `E-SYNTH-NAME-008`). Members exist once
  something references `<N>.<member>`.
- Diagnostics: `E-SYNTH-MODULE-001` … `-007` (see
  `docs/diagnostics/`).

## Bus export

A declared bus exports as a KiCad `(bus_alias "N" (members …))` on
every sheet that carries one of its members, so KiCad's bus tools know
the member set. Member connectivity is by **label name**: each member
net is labelled with its full `<bus>.<member>` name (an explicit
exception to the pin-derived-token policy, which would otherwise render
a guessed `SDA` and collide across two buses). No bus *graphic* is
drawn — the exporter is label-driven for nets too, so a floating bus
wire would only give ERC something to flag.

Verified end to end: a two-instance design with a shared `I2C0` bus
exports, and `kicad-cli sch erc` reports **0 violations** while the
KiCad netlist places `CH1_U1`, `CH2_U1`, both pull-ups, and the bound
MCU pin on the single net `/I2C0.sda`.

## Scope

Implemented here: modules with typed ports and parameters, refdes-prefix
instantiation, interface bundles with one-binding connect, `bind` of a
bus to a part, and bus declaration with KiCad `bus_alias` export.
Deferred (documented above): complex-hierarchy file sharing, nested
module instances, and bus-member wildcards.

Fixtures and tests: `fixtures/ir/module_divider.synth` (snapshot),
`crates/synth-ir/src/modules.rs` (expansion units), and
`crates/synth-ir/tests/modules.rs` (shared-net behaviour + all seven
diagnostics).
