# SPICE simulation (`synth sim`)

`synth sim` compiles a `.synth` design, emits a SPICE netlist from its
simulated parts, runs it through [ngspice](https://ngspice.sourceforge.io/)
and reports each named measurement against its declared limits. Simulation is
opt-in: `validate`, `export` and `place` never run ngspice.

The engine is the external `ngspice` binary. Synth does not link libngspice
and ships no ngspice code.

## Install

```sh
sudo apt install ngspice      # Debian/Ubuntu
brew install ngspice          # macOS
choco install ngspice         # Windows
```

The binary is looked up from `NGSPICE` first, then `PATH`. CI installs
ngspice on Linux and runs the real-engine tests; locally those tests skip with
a message when it is absent. The rest of the suite stays green.

## CLI

```sh
synth sim examples/divider/divider.synth
```

The design must have a testbench sidecar beside it, named
`<design>.sim.toml` (for `divider.synth`, that is `divider.sim.toml`).
The command prints one line per measure:

```
PASS vmid = 6 (min 5.7, max 6.3)
MEASURED bias = 1.65
FAIL iled = 0.026 (min 0.019, max 0.021)
```

`MEASURED` means the measure declared no limits; it has nothing to pass or
fail. A blocking diagnostic (a failed or missing measurement, a missing model,
missing ngspice, a timeout, a malformed testbench, …) makes the command exit
non-zero. A passing limit is visible as a `PASS` line; the MCP report also
carries it as an informational `E-SYNTH-SIM-004` diagnostic.

### Drafting a testbench (`--suggest`)

```sh
cp examples/divider/divider.synth /tmp/divider.synth
synth sim --suggest /tmp/divider.synth
```

`--suggest` compiles the design, recognises a few circuit shapes, and writes
`<design>.sim.toml` from values that carry a source. It never runs ngspice and
never guesses a voltage. An existing sidecar is left alone unless `--force` is
passed (the shipped examples already have hand-written sidecars):

```sh
synth sim --suggest --force /tmp/divider.synth
```

The v1 shapes are:

- **LED plus series resistor.** The current band is
  `operating_conditions.forward_current_ma` ±5%, with the upper limit capped by
  `max_current_ma` when the part declares it. The LED part must carry a
  non-empty `[provenance].datasheet_url`; otherwise no measure is drafted. The
  measure comment names the part id, the operating conditions and the datasheet
  URL. A part without `reviewed_by` gets a `# UNREVIEWED` comment, because the
  registry entry itself has not been reviewed.
- **Resistor divider.** The midpoint is measured against
  `Vsupply * Rbottom/(Rtop+Rbottom)` from the two design `value` fields, ±5%.
  No datasheet is required for a design `value`, and the comment names both
  references, part ids and values.
- **RC low-pass.** The `relative_db` corner is measured at
  `fc = 1/(2*pi*R*C)` from the design `value` fields, ±2%, with the default
  3 dB drop. The AC sweep is `fc/100 .. fc*100`.

Shape names come from the compiled `Board`: exactly one net must be classified
as ground. Every TOML number cites its source in the comment above it. A supply
is the net's declared `power` voltage, or a `PowerOutput` pin that declares an
explicit `voltage_nominal_v`. A `voltage_max_v` rating is never used as a
nominal, and `[operating_conditions]` min/max values are ratings rather than a
rail. A part whose output pin supplies the testbench is replaced by an ideal
source; if its other nets then have no DC path, `synth sim` reports
`E-SYNTH-SIM-008`, and adding the part to `[ignore]` is the usual fix when the
missing model is a known gap.

When a net has no declared supply, the number is never defaulted. If another
shape drafts at least one measure, the file carries a `# TODO` naming that net
and the `power "<net>" <voltage>` line to add. If no measure can be drafted at
all, no file is written and the `not covered` line explains why.

A supply used for an `ac` shape is the AC magnitude too; the corner is
independent of the drive level. Anything the draft does not cover is printed
to stderr as `not covered: <reason>` (a part with no datasheet, a part that is
not one of the shapes, an RC corner that cannot share the file, …). A design
with none of the shapes writes no file and says so. One analysis is selected
per testbench: op shapes (LED, divider) win, and an RC shape is then listed as
not covered because an `op` and an `ac` measure cannot share one file.

The generated values are a starting point. Review every limit against the
datasheet before relying on a passing simulation.

## MCP

`synth_simulate` is a thin wrapper over the same seam. It requires
`file_path` because the sidecar sits beside the design; inline `source` is not
supported. Optional fields are `testbench_path`, `registry_path` and
`workspace_root`. The ngspice binary is taken from `NGSPICE`, then `PATH`;
the tool has no argument for it, so a remote caller cannot choose a command.

```jsonrpc
--> { "jsonrpc": "2.0", "id": 4, "method": "tools/call",
      "params": { "name": "synth_simulate",
                  "arguments": { "file_path": "examples/divider/divider.synth" } } }

<-- { "jsonrpc": "2.0", "id": 4,
      "result": { "passed": true,
                  "measures": [ { "name": "vmid", "value": 6.0,
                                  "min": 5.7, "max": 6.3, "passed": true } ],
                  "diagnostics": [ ... ] } }
```

## Testbench reference

The sidecar is TOML. Unknown fields are rejected. All names are Synth net and
reference names; Synth translates them to sanitized SPICE names.

```toml
ground = "GND"

[[source]]
net = "IN"
kind = "dc"
dc = 0.0
ac = 1.0

[analysis.ac]
start = 10.0
stop = 100000.0
points_per_decade = 100

[[measure]]
name = "corner"
kind = "relative_db"
node = "OUT"
min = 1560.0
max = 1620.0

[options]
timeout = 10.0
plot = "wave.dat"
```

### Top level

| Field     | Meaning                                                          |
| --------- | ---------------------------------------------------------------- |
| `ground`  | Synth net that maps to SPICE node `0`. Required.                  |
| `source`  | Independent voltage sources (`[[source]]`). At least one.         |
| `analysis`| Exactly one of `op` or `ac`. Required.                            |
| `measure` | Named measurements (`[[measure]]`). At least one.                 |
| `options` | Run-wide settings; the table is optional.                         |
| `ignore`  | Placed references to leave out of the deck and topology check.    |

### `[[source]]`

An independent voltage source from the net to ground.

| Field  | Meaning                                                          |
| ------ | ---------------------------------------------------------------- |
| `net`  | Synth net to drive. Must exist and must not be the ground net.    |
| `kind` | Source kind. Only `dc` is accepted today.                         |
| `dc`   | DC value in volts.                                                |
| `ac`   | AC magnitude in volts for an `ac` analysis.                       |

A source must have at least one of `dc` or `ac`; each present value must be
finite. `ac` must be non-negative, but a `relative_db` measure needs a source
with `ac` **greater than 0** (`ac = 0` gives the sweep nothing to excite). A
source with `ac` but no `dc` emits `V<N> <node> 0 AC <value>`; the DC operating
point then has no source drive.

### `[analysis]` and `[analysis.ac]`

Exactly one analysis must be selected.

```toml
[analysis]
op = true
```

or

```toml
[analysis.ac]
start = 1.0
stop = 1000000.0
points_per_decade = 100
```

| Field               | Default | Rule                                  |
| ------------------- | ------- | ------------------------------------- |
| `start`             | `1.0`   | finite and greater than 0             |
| `stop`              | `1e6`   | finite and greater than `start`       |
| `points_per_decade` | `100`   | integer from 1 to 1000                |

The emitted command is `ac dec <points> <start> <stop>`, with values as plain
exponents.

### `[[measure]]`

| Field     | Meaning                                                          |
| --------- | ---------------------------------------------------------------- |
| `name`    | Sentinel name: letters, digits, `_`, `-` or `.`. Unique.         |
| `kind`    | `node_voltage`, `current` or `relative_db`.                      |
| `node`    | Synth net name. Required by `node_voltage` and `relative_db`.    |
| `element` | Synth reference. Required by `current`.                          |
| `drop_db` | Relative drop in dB for `relative_db`. Defaults to `3.0`.        |
| `min`     | Inclusive lower limit. Optional.                                 |
| `max`     | Inclusive upper limit. Optional.                                 |

Rules per kind:

- `node_voltage` reads `v(<node>)` after an `op` analysis. It rejects
  `element` and `drop_db`.
- `current` reads a device current after an `op` analysis. It requires
  `element` and rejects `node` and `drop_db`. The part must have a built-in
  model: resistors, capacitors and inductors emit `@<element>[i]`, diodes and
  LEDs emit `@<element>[id]`. The emitted element name is the model's type
  letter plus the sanitized reference (`LED1` becomes `DLED1`). An ignored
  part, an unknown reference, a registry subcircuit or an unmodelled part is
  `E-SYNTH-SIM-007`, because there is no built-in device current to read.
- `relative_db` measures a corner in an `ac` analysis:

  ```
  meas ac synth_max0 max vdb(n_OUT)
  let synth_target0 = synth_max0 - 3e0
  meas ac synth_corner0 when vdb(n_OUT) = synth_target0
  echo SYNTH_MEAS corner $&synth_corner0
  ```

  `drop_db` defaults to 3.0 and must be finite and greater than 0. It rejects
  `element`. If the crossing cannot be found, ngspice reports
  `Error: measure ...` and the measure fails as `E-SYNTH-SIM-003`.

`min` must not exceed `max`. Limits are inclusive and compared at ngspice's
print precision (see below).

### `[options]`

| Field     | Meaning                                                          |
| --------- | ---------------------------------------------------------------- |
| `timeout` | Wall-clock budget in seconds. Default 10, must be > 0 and ≤ 3600.|
| `plot`    | `wrdata` output file. Requires the `ac` analysis.                |

`plot` must be non-empty and must not contain a newline or a single quote.
A relative path is resolved against the sidecar's directory; an absolute path
is used as is. Synth creates the output directory before ngspice starts, so a
missing directory cannot silently lose the plot. The deck emits
`wrdata '<absolute path>' allv` after the measures and before `SYNTH_DONE`.
ngspice 42 does not accept a double-quoted `wrdata` path; single quotes do.
`allv` writes every vector in the current plot, one row per frequency point.

### `ignore`

```toml
ignore = ["U1", "Y1"]
```

Ignored parts are left out of the deck and the topology check. This is for a
known model gap, not for hiding an accident: removing the only DC path to a
net reports `E-SYNTH-SIM-008`.

## How the run works

Synth emits a deck with one line per simulated part, then a `.control` block.
It runs the selected analysis explicitly, computes each measure, prints
`echo SYNTH_MEAS <name> <value>` per measure and `echo SYNTH_DONE` last. Only
those sentinel lines are parsed; `.meas` text is used only for the
`relative_db` crossing.

- The run happens in a fresh temporary directory with its own `.spiceinit`
  (`set strict_errorhandling`, plus the registry model dialect when one is
  used), so a user's `~/.spiceinit` and concurrent runs cannot interfere.
- Model `.include` paths are absolute and quoted. Model files may not contain
  `.control`, `.endc`, `shell`, `system`, nested `.include`/`.inc`/`.lib`, or
  Verilog-A/encrypted content.
- The default timeout is 10 seconds. On timeout the process tree is killed and
  the run fails with `E-SYNTH-SIM-005`.
- Every node needs at least two connections and a DC path to ground before
  the run starts (`E-SYNTH-SIM-008`). Synth never turns on `rshunt` silently.
- A missing measurement, a missing `SYNTH_DONE`, an unparseable or non-finite
  value, a failed `.meas`, a duplicate sentinel or an unexpected sentinel is
  an error. A value is never defaulted to 0, so a broken simulation can never
  read as a silent pass.

### Print precision

ngspice prints measured values with six significant digits. Synth compares a
value with its limits at that print precision: a difference of no more than
half a unit in the last printed digit counts as equal to the limit, so
`6.000004` against `min = 6` passes while `6.00001` fails. The reported value
is the printed value.

## Examples

| Example | What it checks |
| ------- | -------------- |
| [divider](../examples/divider/divider.synth) | `V_MID = 12 × 10k/(10k+10k) = 6 V` with an `op` analysis. |
| [rc_filter](../examples/rc_filter/rc_filter.synth) | `fc = 1/(2π·1k·100n) = 1591.55 Hz`; the -3 dB crossing is 1587.8 Hz. |
| [led](../examples/led/led.synth) | `I = (5 − 2.0)/150 = 20 mA`, measured as the `LED1` branch current. |

```sh
synth sim examples/divider/divider.synth
synth sim examples/rc_filter/rc_filter.synth
synth sim examples/led/led.synth
```

## Diagnostics

Each simulation failure is a stable code with a page under
[`docs/diagnostics`](diagnostics/README.md):

| Code | Meaning |
| ---- | ------- |
| `E-SYNTH-SIM-001` | ngspice not found. |
| `E-SYNTH-SIM-002` | A part has no SPICE model. |
| `E-SYNTH-SIM-003` | A measurement is missing, failed, non-finite or unparseable. |
| `E-SYNTH-SIM-004` | A measurement is outside its limit (info when inside). |
| `E-SYNTH-SIM-005` | ngspice exceeded the time budget. |
| `E-SYNTH-SIM-006` | ngspice reported an error unrelated to a measure. |
| `E-SYNTH-SIM-007` | Malformed testbench or name collision. |
| `E-SYNTH-SIM-008` | A node is floating or has no DC path. |

## Not covered

- Verilog-A and encrypted vendor models (ngspice cannot run them; Synth
  reports an error instead of guessing).
- RF, S-parameter and EMC simulation.
- Firmware co-simulation and mixed-signal simulation.
- Switching converters beyond a few milliseconds of simulated time under the
  10-second default budget.
- Per-part SPICE dialect mixing: all parts in one run must use the same
  `[spice] dialect`, because ngspice has one `ngbehavior` setting per run.
- Transient and DC-sweep analyses, pulse/pwl/sin sources, and expression
  measures.
