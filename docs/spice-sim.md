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

- **LED plus series resistor.** The current band is the built-in diode
  equation `I*R + Vf0 + N*Vt*ln(I/If0) = Vs` ±5%, where `Vs` is the supply net
  voltage, `Vf0`/`If0` are `operating_conditions.forward_voltage_v`/`_current_ma`
  and `R` is the design series resistor's `value`. The registry Vf is the value
  at `If0`, while the model's Vf is lower at low current, so a fixed
  `(Vs - Vf)/R` band would sit too low. The
  upper limit is capped by `max_current_ma`, or by `forward_current_ma` when the
  part declares no separate maximum. A part without `forward_voltage_v` or
  `forward_current_ma` gets no measure (the built-in model needs both). When the
  expected current is above the cap, the drafted band is pinned to the cap so
  the simulation fails, and the stderr line starts with `overdrive:`.
  The LED part must carry a non-empty `[provenance].datasheet_url`; otherwise
  no measure is drafted. The measure comment names the resistor, the part id,
  the operating conditions and the datasheet URL. A part without `reviewed_by`
  gets a `# UNREVIEWED` comment, including a part with no `[provenance]` block,
  because the registry entry itself has not been reviewed.
  When the supply is an output-capable IC pin carrying `[pins.drive]`, the
  draft writes a `pin` source at `level = "high"` instead of a net source, adds
  `output_resistance_ohm` to the series resistance in the hand-check, and adds
  a second `pin_current` measure for the pin's `max_source_ma`. The comments
  cite the pin, `high_v`, `output_resistance_ohm` when present, and the
  `datasheet_ref`. The IC is ignored as the supply driver. A pin with no drive
  data, drive data without `datasheet_ref`, or several driven pins on the net
  gets no draft and a not-covered line containing `pin has no drive data` (or
  naming the ambiguity); no value is guessed and no TODO is written for it.
- **Resistor divider.** The midpoint is measured against
  `Vsupply * Rbottom/(Rtop+Rbottom)` from the two design `value` fields, ±5%.
  No datasheet is required for a design `value`, and the comment names both
  references, part ids and values. Derived from this design's values; checks
  wiring and model, not the value choice.
- **RC low-pass.** The `relative_db` corner is measured at
  `fc = 1/(2*pi*R*C)` from the design `value` fields, ±2%, with the default
  3 dB drop. The AC sweep is `fc/100 .. fc*100`. Two `node_voltage` measures
  also pin the absolute gain at `fc/10` (-0.5..0 dB) and `10*fc`
  (-23..-17 dB): a single pole gives -0.043 dB and -20.04 dB there, while a
  swapped R/C (a high-pass with the same corner) does not. The draft drives
  the AC sweep at 1 V for every source, so `vdb(node)` is an absolute gain in
  dB. Derived from this design's values; checks wiring and model, not the
  value choice. Every `at_frequency` sits at or above the sweep `start` and
  below `stop`; the stop point itself is rejected because ngspice cannot
  interpolate there.

Shape names come from the compiled `Board`: exactly one net must be classified
as ground. Every TOML number cites its source in the comment above it. A supply
is the net's declared `power` voltage, a `PowerOutput` pin that declares an
explicit `voltage_nominal_v`, or exactly one output-capable pin (`output`,
`three_statable`, an open-drain type or `bidirectional`) that carries
`[pins.drive]`; the pin supply uses `high_v` and, when present,
`output_resistance_ohm`. If more than one driven pin sits on the net, the
supply is ambiguous and none is guessed. A `voltage_max_v` rating is never used
as a nominal, and `[operating_conditions]` min/max values are ratings rather
than a rail. A part whose output pin supplies the testbench is replaced by an
ideal source, and the draft adds it to `ignore` with a comment saying why. Any
other unused part with no built-in model is also added to `ignore` (still
reported on stderr as not covered); if an ignored part leaves a net with no DC
path, `synth sim` reports `E-SYNTH-SIM-008`.

A part with a bad value (an unparseable `value`, or a zero/negative passive) is
never added to `ignore`: hiding the typo would turn a hard `E-SYNTH-SIM-002`
into a tempting green run. When an ignored part kills a rail, every passive
that would otherwise keep that dead rail alive is ignored as a group, so two
decoupling capacitors on the same dead rail do not keep each other "live". A
modelled regulator that stays in the deck needs its input driven: when its
input net has no source and a `power` declaration exists, the draft sources it
from that voltage; otherwise it writes a `# TODO` for the net.

When a net has no declared supply, the number is never defaulted. If another
shape drafts at least one measure, the file carries a `# TODO` naming that net
and the `power "<net>" <voltage>` line to add. A net driven by another resistor
is an internal node of a longer chain: it gets no TODO, but the divider is
reported as not covered. If no measure can be drafted at all, no file is
written, the reason goes to stderr as a `not covered` line and the command still
exits 0; callers must check that the file exists.

A supply used for an `op` shape is that net's DC value. An `ac` draft drives
the source at 1 V regardless of the declared supply, so an absolute `vdb`
measure reads gain in dB. Anything the draft does not cover is printed
to stderr as `not covered: <reason>` (a part with no datasheet, a part that is
not one of the shapes, an RC corner that cannot share the file, …). A drafted
overdriven LED is a warning printed as `overdrive:`, not a not-covered line. A
design with none of the shapes writes no file and says so. One analysis is
selected per testbench: op shapes (LED, divider) win, and an RC shape is then
listed as not covered because an `op` and an `ac` measure cannot share one file.

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

A source drives exactly one target: a net with `dc`/`ac`, or an IC pin's
output with `level`. Mixing the two forms, giving a pin `dc`/`ac`, or giving a
net a `level` is `E-SYNTH-SIM-007`.

| Field   | Meaning                                                          |
| ------- | ---------------------------------------------------------------- |
| `net`   | Synth net to drive. Must exist and must not be the ground net.    |
| `pin`   | `REF.PIN` of an output pin carrying `[pins.drive]`.              |
| `level` | `high` or `low` for a pin source. Required with `pin`.            |
| `kind`  | Source kind. Only `dc` is accepted today; defaults to `dc`.      |
| `dc`    | DC value in volts. Required for a net source unless `ac` is set.  |
| `ac`    | AC magnitude in volts for an `ac` analysis.                       |

A net source must have at least one of `dc` or `ac`; each present value must
be finite. `ac` must be non-negative, but a `relative_db` measure needs a
source with `ac` **greater than 0** (`ac = 0` gives the sweep nothing to
excite). A source with `ac` but no `dc` emits `V<N> <node> 0 AC <value>`; the
DC operating point then has no source drive.

A net may be declared by only one source. Two sources that resolve to the same
design net (net plus pin, or two pins) are `E-SYNTH-SIM-007` naming both source
descriptions and the net; the second is left out of the deck.

### Driving an IC pin

A registry pin may carry the datasheet's output-drive data:

```toml
[[pins]]
name = "gpio5"
number = "8"
electrical_type = "bidirectional"

[pins.drive]
high_v = 3.3                     # output level at no load
low_v = 0.0
max_source_ma = 4.0              # datasheet current limits
max_sink_ma = 4.0
output_resistance_ohm = 25.0     # optional; absent means an ideal source
datasheet_ref = "Table 5-1"      # required
```

The testbench then drives that pin's net at a chosen level:

```toml
[[source]]
pin = "U1.gpio5"
level = "high"
```

Synth finds the placed part by reference and the pin by name, finds the one net
the pin is on, and emits a voltage source at `high_v` (or `low_v`). When
`output_resistance_ohm` is present it emits an internal node plus a series
resistor (`V<N> vpin_<N> 0 DC …` and `Rpin<N> n_<net> vpin_<N> <rout>`); the
generated resistor joins the element-name collision check, so a design
reference that would emit as the same name is an error rather than a silent
short. Without it the source is ideal: `V<N> n_<net> 0 DC <level voltage>`.

The pin source is treated exactly like a net source by the topology check: it
is a DC path from its net to ground. An unknown reference, an unknown pin, a
pin on no net, a pin on the ground net and two sources on one net are all
`E-SYNTH-SIM-007`. A pin with no `[pins.drive]` block, or a drive block without
a non-empty `datasheet_ref`, is `E-SYNTH-SIM-002` naming the pin; a missing
value is never defaulted. The IC that owns the pin must still be listed under
`ignore` (or carry a `[spice]` model): drive data says what the pin does, not
how the rest of the IC behaves.

PWM, transient output changes and firmware behaviour are **not** simulated:
this is one DC operating point at the chosen level. A GPIO that toggles, a
pin whose level is set by software, and any switching waveform need a future
transient source; do not read a passing DC check as a timing guarantee.

The matching measure reads the current out of that source in milliamps:

```toml
[[measure]]
name = "pin_current"
kind = "pin_current"
pin = "U1.gpio5"
```

There is no `min`, `max`, `node`, `element`, `drop_db` or `at_frequency` on
this measure: the limit comes from the registry and the source level —
`max_source_ma` for `level = "high"`, `max_sink_ma` for `level = "low"`. The
deck saves `@V<N>[i]` and computes `abs(@V<N>[i]) * 1000`. A passing check is
an informational `E-SYNTH-SIM-004`; a failure is blocking `E-SYNTH-SIM-004`
naming the pin, the measured milliamps, the datasheet field, the limit and the
`datasheet_ref`. The measure requires the `op` analysis and a `[[source]]` with
the same pin; a missing source is `E-SYNTH-SIM-007`.

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
exponents. The sweep must contain at least two points:
`points_per_decade * log10(stop/start) >= 1`. A smaller sweep (for example
`.ac dec 1 10 55`) makes real ngspice 42 hang until the timeout, so it is
`E-SYNTH-SIM-007` naming the range and telling you to widen the sweep or raise
`points_per_decade` before any process starts.

### `[[measure]]`

| Field     | Meaning                                                          |
| --------- | ---------------------------------------------------------------- |
| `name`    | Sentinel name: letters, digits, `_`, `-` or `.`. Unique.         |
| `kind`    | `node_voltage`, `current`, `relative_db` or `pin_current`.       |
| `node`    | Synth net name. Required by `node_voltage` and `relative_db`.    |
| `element` | Synth reference. Required by `current`.                          |
| `pin`     | `REF.PIN`. Required by `pin_current`.                            |
| `drop_db` | Relative drop in dB for `relative_db`. Defaults to `3.0`.        |
| `at_frequency` | Frequency in Hz for an absolute gain on `node_voltage`.     |
| `min`     | Inclusive lower limit. Optional.                                 |
| `max`     | Inclusive upper limit. Optional.                                 |

Rules per kind:

- `node_voltage` reads `v(<node>)` after an `op` analysis. It rejects
  `element` and `drop_db`. With `at_frequency` it instead reads an absolute
  gain during an `ac` analysis:

  ```
  meas ac synth_gain0 find vdb(n_OUT) at=1.59e2
  echo SYNTH_MEAS passband $&synth_gain0
  ```

  `at_frequency` must be positive and finite, must be at least `start` and less
  than `stop` (the stop point has no interval, so ngspice cannot interpolate
  there), requires the `ac` analysis and a source with `ac` greater than 0, and
  is rejected on `relative_db`, `current` and `pin_current`. The result is
  `20*log10(|V(node)|)` in dB relative to a 1 V source, so a draft drives the
  AC source at 1 V.
- `current` reads a device current after an `op` analysis. It requires
  `element` and rejects `node`, `drop_db` and `at_frequency`. The part must
  have a built-in model: resistors, capacitors and inductors emit
  `@<element>[i]`, diodes and LEDs emit `@<element>[id]`. The emitted element
  name is the model's type letter plus the sanitized reference (`LED1`
  becomes `DLED1`). An ignored part, an unknown reference, a registry
  subcircuit or an unmodelled part is `E-SYNTH-SIM-007`, because there is no
  built-in device current to read.
- `pin_current` reads the current out of an IC pin source, in milliamps, after
  an `op` analysis. It requires a non-empty `pin` (`REF.PIN`) and rejects
  `node`, `element`, `drop_db`, `at_frequency`, `min` and `max`: the limit is
  the pin's datasheet `max_source_ma` (level `high`) or `max_sink_ma` (level
  `low`), never a number in the sidecar. The pin must have a matching
  `[[source]]`; otherwise the measure is `E-SYNTH-SIM-007`. The deck emits
  `save all @V<N>[i]` and `let m<index> = abs(@V<N>[i]) * 1000`, so the reported
  value and the `E-SYNTH-SIM-004` message are milliamps. The failure names the
  pin, the measured current, the datasheet field and limit, and the
  `datasheet_ref`; a pass is informational.
- `relative_db` measures a corner in an `ac` analysis:

  ```
  meas ac synth_max0 max vdb(n_OUT)
  let synth_target0 = synth_max0 - 3e0
  meas ac synth_corner0 when vdb(n_OUT) = synth_target0
  echo SYNTH_MEAS corner $&synth_corner0
  ```

  `drop_db` defaults to 3.0 and must be finite and greater than 0. It rejects
  `element` and `at_frequency`. If the crossing cannot be found, ngspice
  reports `Error: measure ...` and the measure fails as `E-SYNTH-SIM-003` with
  a message naming the Synth measure, node and drop band.

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
| [rc_filter](../examples/rc_filter/rc_filter.synth) | `fc = 1/(2π·1k·100n) = 1591.55 Hz`; the -3 dB crossing is 1587.8 Hz, with gain -0.04 dB at 159 Hz and -20.0 dB at 15.9 kHz. |
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

- PWM, GPIO firmware and transient behaviour of an IC pin source: the pin
  drive is one DC operating point at the chosen level.
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

## Known limits

- A header-looking line inside a `"""` string can cut a measure span short.
- A kept bad-value part can add extra `E-SYNTH-SIM-008` noise from its ignored
  neighbours.
- A ferrite with no value is a hard `E-SYNTH-SIM-002`.
- An AC draft whose supply is a modelled regulator is skipped.
