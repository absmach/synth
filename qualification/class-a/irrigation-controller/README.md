# Irrigation controller

Two-zone irrigation controller: two soil-moisture probes, a flow meter, two
solenoid valve outputs, and ambient temperature, humidity and light.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 111.90 × 95.40 mm |
| Layers | 2 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill) |
| Components | 64 |
| Source | `irrigation_controller.synth` |
| Sidecars | `irrigation_controller.placement.layout.toml`, `irrigation_controller.schematic.layout.toml` |

## Supply

| Rail | Source | Nominal | Feeds |
| --- | --- | --- | --- |
| 12 V in | J1, protected | 12 V | valve coils, buck input |
| 12 V protected | F1 → Q1 | 12 V | J4, J5 coil supply |
| 3V3 | U2 MP2307 buck | 3.3 V | MCU, sensors, EEPROM, probe gate, pull-ups |
| Probe excitation | U7 TPS22918, gated | 3.3 V | J6, J7 pin 1 only |

Input protection, in order from the connector: F1 resettable polyfuse
(1.1 A hold), Q1 P-channel MOSFET for reverse polarity, D1 5V1 zener
clamping Q1's gate, D2 TVS array on the rail and return.

## Connectors

All pinouts below are derived from the source, not transcribed.

**J1 — 12 V input, JST XH 2-pin**

| Pin | Signal |
| --- | --- |
| 1 | +12 V in (to F1) |
| 2 | GND |

**J4 — valve zone 1, JST XH 2-pin** · **J5 — valve zone 2, JST XH 2-pin**

| Pin | Signal |
| --- | --- |
| 1 | +12 V protected (coil high side) |
| 2 | Coil return, switched low-side by Q2 (J4) / Q3 (J5) |

**J6 — soil probe zone 1, JST PH 3-pin** · **J7 — soil probe zone 2, JST PH 3-pin**

| Pin | Signal |
| --- | --- |
| 1 | Switched 3V3 excitation (U7 output) |
| 2 | Analogue return → 10 kΩ → PA1 (J6) / PA2 (J7) |
| 3 | GND |

**J8 — flow meter, JST PH 3-pin**

| Pin | Signal |
| --- | --- |
| 1 | +3V3 |
| 2 | Pulse input → PA3, pulled up 10 kΩ, 100 nF to GND |
| 3 | GND |

**J3 — SWD, 1×4 header 2.54 mm**

| Pin | Signal |
| --- | --- |
| 1 | +3V3 |
| 2 | SWDIO (PA13) |
| 3 | SWCLK (PA14) |
| 4 | GND |

**J2 — spare I²C, JST PH 4-pin**

| Pin | Signal |
| --- | --- |
| 1 | +3V3 |
| 2 | SDA (PB7) |
| 3 | SCL (PB6) |
| 4 | GND |

## MCU pin assignment

STM32G030K8T6, LQFP-32. Thirteen of its pins are used.

| Pin | Direction | Function |
| --- | --- | --- |
| PA1 | analogue in | Soil probe zone 1 (via 10 kΩ / 100 nF) |
| PA2 | analogue in | Soil probe zone 2 (via 10 kΩ / 100 nF) |
| PA3 | digital in | Flow meter pulse, pulled up |
| PA4 | digital out | Valve zone 1 gate (via 100 Ω) |
| PA5 | digital out | Valve zone 2 gate (via 100 Ω) |
| PA6 | digital out | Status LED, irrigating |
| PA13 | SWD | SWDIO, to J3 |
| PA14 | SWD | SWCLK, to J3 |
| PB0 | digital out | Probe excitation enable (U7 ON) |
| PB6 | I²C | SCL |
| PB7 | I²C | SDA |
| PC14 | OSC | Crystal Y1 |
| PC15 | OSC | Crystal Y1 |
| NRST | reset | SW1, pulled up 10 kΩ, debounced 100 Ω / 100 nF |

Every other GPIO is unconnected. On an LQFP-32 with this function set there
is no header budget for them.

## I²C bus

One bus, 4k7 pull-ups (R5/R6) to 3V3, no other pull-ups on the board.

| Address | Device | Function |
| --- | --- | --- |
| 0x29 | U5 APDS-9301 | Ambient light, ADR_SEL tied low |
| 0x40 | U4 HDC1080 | Air temperature and humidity |
| 0x50 | U6 24LC256 | Schedule and calibration, WP tied low |

The EEPROM's A0–A2 are tied low and the APDS-9301's ADR_SEL is tied low,
both in the source. ADR_SEL also reaches 0x39 floating and 0x49 high, so that
address is a decision rather than a default. The HDC1080 has no strap: 0x40 is
fixed in silicon.

## Outputs

| Output | Device | Drive | Protection |
| --- | --- | --- | --- |
| Valve zone 1 | Q2 IRLML6244 | low-side, PA4 | D3 1N5819 flyback, R11 100 kΩ gate pull-down |
| Valve zone 2 | Q3 IRLML6244 | low-side, PA5 | D6 1N5819 flyback, R13 100 kΩ gate pull-down |

Both valves are closed whenever the MCU is in reset, unpowered, or driving
its pin low — the gate pull-downs hold them off. R11 and R13 are what make
that true and must not be omitted.

## Netclasses

| Class | Track | Clearance | Applies to |
| --- | --- | --- | --- |
| POWER | 0.6 mm | 0.25 mm | **no net** |
| VALVE | 0.6 mm | 0.3 mm | **no net** |
| I2C | 0.15 mm | 0.15 mm | **no net** |

Declared but inert: a `netclass` block assigns nothing, and nets join a class
through `class "NAME"` on their `connect`, `net` or `power` statement. No net
on this board carries one, so every width and clearance above is unapplied
and the board routed at the defaults. True of every board in this corpus.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U3 | STM32G030K8T6 | `stm32g030` |
| U2 | MP2307 | `mp2307_buck` |
| U7 | TPS22918 | `tps22918_loadswitch` |
| U4 | HDC1080DMBR | `hdc1080_humidity` |
| U5 | APDS-9301 | `apds9301_lux` |
| U6 | 24LC256-I/SN | `24lc256_eeprom` |
| Y1 | 8 MHz crystal | `xtal_8mhz` |
| Q1 | Si2301 | `p_mosfet_si2301` |
| Q2, Q3 | IRLML6244 | `n_mosfet_irlml6244` |
| D2 | TVS array SOT-363 | `tvs_array_5v` |
| D3, D6 | 1N5819 | `d_schottky` |

The generated `bom.csv` is the bill of materials; this table is the actives
and omits twenty-six generic passives. Twenty of the board's parts carry no
distributor identity, so neither can be ordered as it stands.

## Measured

One local run of the export and the external router. No figure here came from
anywhere else.

| | |
| --- | --- |
| Nets closed | **34 of 34** |
| Open | **0** |
| Routed copper | 1968.2 mm over 392 segments and 87 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, 19 warnings |

Three consecutive exports produce a byte-identical board and schematic.

This is the largest board in the Class-A set and the only one that routes with
no DRC error at all. Size is not what decides it: every package here escapes
on the standard floor, which the 0.4 mm pitch QFN on the debug target does
not.

### The nineteen warnings

| Finding | Count |
| --- | --- |
| silkscreen over copper | 12 |
| library footprint mismatch | 7 |

Both are advisory. The silkscreen warnings are reference designators crossing
pads, cosmetic on a fabricated board and fixed in a silkscreen pass Synth does
not perform. The footprint-mismatch warnings compare the board's embedded
footprints against the installed libraries and depend on library version, so
they should be read from CI rather than a local run.

## Limitations

- **Two vias land inside pads** and via-in-pad is not declared, so the package
  is refused despite the clean DRC. That is the one thing standing between
  this board and a fabricable result.
- No Gerbers, drill, position file or STEP: the export stops short of
  fabrication artifacts while the board is not fabrication-ready.
- **The analogue path is unverified.** `stm32g030` declares no
  `analog_input` capability, so PA1 and PA2 are ordinary GPIO to validation.
  Reference, source impedance and settling against the 10 kΩ filter are a
  reviewer's job.
- No thermal analysis: the buck and both FETs switching inductive loads each
  warrant one.
- The netclasses above apply to nothing (see *Netclasses*).
- `E-SYNTH-POWER-004` reports the 3V3 rail carrying more load than its
  decoupling count supports. 24 pads and no plane, because a declared plane
  class pours ground only on this board.
- Twelve `E-SYNTH-SCHEM-003` warnings quote schematic separations that are
  not credible at this board size; they depend on KiCad symbol geometry and
  should be read from CI, not from a local run.
- `E-SYNTH-SCHEM-010` wants an explicit rail name instead of `VDD`;
  SynthSpec V1 has no net-naming syntax, so it cannot be acted on.
- Divider ratio and the 4k7 pull-up value are datasheet and convention, not
  measurement.

## Reproducibility

The placement sidecar is a fixed point: the placer reproduces its 64
coordinates exactly and two runs agree. Reaching that took one iteration,
because edge legalisation runs after a `hard` override and moves
edge-adjacent parts.

A via count is not a fingerprint — the board name seeds the deterministic
uuids, so renaming changes the order the router works in. Compare
connectivity.
