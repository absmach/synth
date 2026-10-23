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
| Components | 67 |
| Nets | 37 |
| Source | `irrigation_controller.synth` |
| Sidecars | `irrigation_controller.placement.layout.toml`, `irrigation_controller.schematic.layout.toml` |

## Supply

| Rail | Source | Nominal | Netclass | Feeds |
| --- | --- | --- | --- | --- |
| 12 V in | J1, through F1 | 12 V | — | Q1 drain |
| `V12`, protected | Q1 source | 12 V | `POWER`, 0.6 mm | J4, J5 coil supply, buck input |
| `V3V3` | U2 MP2307 buck | 3.34 V (3.25–3.43 V) | default | MCU, sensors, EEPROM, probe gate, pull-ups |
| Probe excitation | U7 TPS22918, gated | 3.3 V | default | J6, J7 pin 1 only |

Both rails are declared in the source with `power`, so the regulator-range and
load-budget rules judge them. `V3V3` is left at the default track width on
purpose: it feeds a 0.8 mm-pitch LQFP-32 and two DFN sensors that a 0.6 mm
track cannot enter.

Input protection, in order from the connector: F1 resettable polyfuse
(1.1 A hold), Q1 P-channel MOSFET for reverse polarity, D1 5V1 zener from
Q1's source to its gate, D2 SMAJ15A transient clamp on the protected rail.

## Power stage

Every value here is from a datasheet's own equation or table and is
**calculated, not measured**. The 12 V stage and the buck were wrong in the
first version of this board and are the reason this section exists.

**Reverse polarity (Q1, R1, D1).** The P-FET has its **drain on the input and
its source on the protected rail**. A P-FET's body diode runs drain to source,
so this points it from the input to the load: forward polarity turns the diode
on, the rail comes up and the channel enhances; reversed polarity reverses the
diode, which blocks, and the gate, held to ground through R1, sits at the
source's potential and keeps the channel off. The first version had the source
on the input, which points the body diode from the load back to the input —
through the load, a reversed supply forward-biased it.

D1 is a 5V1 zener from source to gate, anode on the gate. With R1 holding the
gate at ground, 12 V on the rail would put 12 V across Vgs; the Si2301's
absolute maximum is ±8 V. The zener holds Vgs to about −5.1 V: fully
enhanced, and inside the limit. The first version put it from gate to ground,
where it does nothing for Vgs.

**Transient clamp (D2).** An SMAJ15A: 15 V standoff clears a 12 V rail, and
its 24.4 V clamp is under the MP2307's 26 V absolute maximum on VIN (Rev 1.9),
though not by much, and above its 23 V operating maximum — a transient at the
clamp is survived, not operated in. The `tvs_array_5v` it replaced is a 5 V
part and would have conducted continuously on this rail.

**Output voltage (R2, R9).** MP2307 Rev 1.9: VOUT = 0.925 × (R1 + R2) / R2
with R1 from the output to FB. Its Table 1 gives 26.1 kΩ over 10 kΩ for 3.3 V,
which is what this board uses: 0.925 × (1 + 26.1/10) = **3.34 V**, and
3.25 to 3.43 V across the datasheet's 0.900 to 0.950 V feedback tolerance.
The first version used 10 kΩ over 3.01 kΩ, which sets 0.925 × (1 + 10/3.01)
= **4.0 V**, above the STM32G030's 3.6 V maximum.

**Compensation (R17, C4).** The datasheet's pin description calls for a series
RC network from COMP to GND. The first version had C4 between FB and COMP,
which is not a compensation network. R17 = 2.32 kΩ and C4 = 15 nF come from
the datasheet's own procedure: R3 < 2π·C2·fC / (GEA·GCS) × VOUT/VFB with
C2 = 22 µF, fC = 20 kHz (under fS/10 = 34 kHz), GEA = 820 µA/V and
GCS = 5.2 A/V gives 2.31 kΩ; C3 > 4 / (2π·R3·fC) gives 13.7 nF. This assumes
the full 22 µF; an X5R 0805 loses a good part of that at 3.3 V of bias, which
would lower the crossover, and the result has not been measured on hardware.

**Enable and start-up (R18, C22).** The datasheet's pin description says to
attach EN to IN through a 100 kΩ pull-up, and its absolute maximum for every
pin except IN, SW and BS is 6 V. EN tied straight to a 12 V rail, as the first
version had it, is outside that. C22 = 100 nF on SS sets a soft start of about
15 ms, per the datasheet. The bootstrap capacitor C3 is 100 nF against a
10 nF minimum.

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

| Class | Asked | Clearance | Net | Routed | Segments |
| --- | --- | --- | --- | --- | --- |
| POWER | 0.6 mm | 0.25 mm | `V12` | **0.6 mm** | 74 |
| VALVE | 0.6 mm | 0.3 mm | `VALVE1`, `VALVE2` | **0.6 mm** | 5, 12 |
| I2C | 0.15 mm | 0.15 mm | `SDA`, `SCL` | **0.2 mm** | 33, 27 |

A `netclass` block assigns nothing by itself; nets join a class through
`class "NAME"` on their `connect`, `net` or `power` statement. The first
version of this board declared three classes and joined no net to any, so all
of it routed at the default.

Measured from the routed board's track widths, per net: `V12` and both valve
nets came out at 0.6 mm, as asked. **The I2C class did not.** The exported
project file states it correctly — class `I2C`, `track_width = 0.15` — and the
router drew 0.2 mm on all 60 segments of `SDA` and `SCL` anyway. The exporter
is not the problem; the router does not go below 0.2 mm for a net class. The
board is not worse for it, since 0.2 mm is above the 0.127 mm process floor,
but the I2C width in the table is a request the router did not honour.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U3 | STM32G030K8T6 | `stm32g030` |
| U2 | MP2307 | `mp2307_buck` |
| U7 | TPS22918 | `tps22918_loadswitch` |
| U4 | HDC1080DMBR | `hdc1080_humidity` |
| U5 | APDS-9301 | `apds9301_lux` |
| U6 | 24LC256-I/SN | `24lc256_eeprom` |
| Y1 | 8 MHz crystal, X32258MOB4SI | `xtal_8mhz` |
| Q1 | Si2301 | `p_mosfet_si2301` |
| Q2, Q3 | IRLML6244 | `n_mosfet_irlml6244` |
| D2 | SMAJ15A TVS | `smaj15a_tvs` |
| D3, D6 | 1N5819 | `d_schottky` |
| F1 | MF-MSMF110/16-2, 1.1 A hold | `polyfuse_1r1a` |
| L1 | CBC3225T100MR, 10 µH | `l_1210_10uh` |

The generated `bom.csv` is the bill of materials. This table lists 15 of the
67 components; the other 52 are passives, connectors, LEDs and a switch. Every
part the supply check covers names a manufacturer part number. Generic
resistors and capacitors are exempt from that check and are specified by value,
so they do not. See *Limitations* for what a part number does and does not
establish.

## Measured

Three consecutive runs of the export and the external router, from an
unchanged source tree, each with `--gerbers --drill --pnp
--allow-unverified-parts` (the last because no registry part records a
reviewer, #51). Every figure below was identical in all three.

| | |
| --- | --- |
| Nets closed | **37 of 37** |
| Open | **0** |
| Routed copper | 2112.0 mm over 574 segments and 105 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **0** |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, **0 warnings** |
| Schematic-parity differences | **0** |
| Routing verdict | `routed`, fabrication-ready |

`kicad-cli pcb drc` reports nothing at any severity it checks. That is the
whole of what it measures: it says nothing about return paths, which is the
first limitation below.

## Fabrication

The package passes the fabrication gate, so the artifacts exist: 20 Gerber
layers plus the job file, Excellon PTH and NPTH drill files with their maps, a
67-row position file and a 67-row BOM.

The gate matters. `synth export-kicad --gerbers` refuses with `E-SYNTH-QUAL-000`
if any part's pin map disagrees with its footprint, and it did refuse during
this work: a copy of the TPS22918's older pin map put the load switch's output
on the wrong pad, and no routing output showed it.

## Reproducibility

| Artifact | Run A | Run B | Run C | |
| --- | --- | --- | --- | --- |
| Schematic, root sheet | `67253e3a5eeb` | `67253e3a5eeb` | `67253e3a5eeb` | identical |
| Schematic, 8 sub-sheets | `6af83e602ed2` | `6af83e602ed2` | `6af83e602ed2` | identical |
| Symbol library | `9c451b25de5a` | `9c451b25de5a` | `9c451b25de5a` | identical |
| BOM | `45b3881ebd9d` | `45b3881ebd9d` | `45b3881ebd9d` | identical |
| Position file | `139e8898a0dd` | `139e8898a0dd` | `139e8898a0dd` | identical |
| Project file | `7c277a56a5b2` | `7c277a56a5b2` | `7c277a56a5b2` | identical |
| Routed board | `0cdde1eedbc1` | `0219326f0550` | `f6736af6a5f9` | **differs** |

**Byte-identical:** the schematic and all eight sub-sheets, the symbol
library, the BOM, the position file and the project file.

**Not byte-identical, and why:**

- **The routed board.** The copper is the same — 679 segments and vias, each
  present the same number of times in all three runs, and the 67 footprints sit
  at identical positions — but the router writes its tracks in a different
  order each run. That is #277.
- **Gerbers.** Eighteen of the twenty layers are identical once KiCad's
  `CreationDate` and "Created by KiCad … date" lines are removed; those four
  lines per file are the only difference in `Edge_Cuts`, the masks and the
  silkscreen. Both Excellon drill files are identical once their date comment
  is removed. `F_Cu` and `B_Cu` are not, and are the two layers that carry the
  track order. I have not established that their geometry is identical beyond
  the PCB's own copper being so.

So the claim this board can make is: identical source, schematic, BOM, copper
and placement across three runs, with the remaining differences explained by a
timestamp KiCad embeds and a track ordering that is #277. It cannot yet make
the claim that every fabrication file hashes the same.

**Placement.** 64 of the 67 parts are pinned in
`irrigation_controller.placement.layout.toml`. `F1`'s pinned y was moved to the
row of `J1`'s pad 1 when the fuse changed footprint; with the old y the router
ended the input net's track 0.04 mm off the pad's rounded corner, which
`kicad-cli` accepts and Synth's own validator does not. The three added in review — the
compensation resistor, the enable pull-up and the soft-start capacitor — are
not, and the placer put them in the same place in all three runs.

A via count is not a fingerprint: the board name seeds the deterministic uuids,
so renaming changes the order the router works in. Compare connectivity.

**Independent review.** None. Nothing in this README is a reviewer's sign-off,
and the board is `experimental` in the qualification matrix (#247) for that reason.

## Limitations

- **Return paths are not addressed.** `W-SYNTH-DRC-016` reports 105
  layer changes with no ground-return via nearby, one for every via on the
  board: no ground via was placed anywhere. 33 of the 105 are on the DC rails,
  15 on I²C, 2 on a valve return. Synth has no way to add one and the check
  does not credit a ground plane; a 4-layer stackup with ground on every layer
  was tried and is recorded in #280, with the figures. This is the open P1
  finding from review, and it is not fixed.
- **The I²C class is not honoured.** It asks for 0.15 mm and the router drew
  0.2 mm; see *Netclasses*.
- **The loop compensation is calculated, not measured.** R17 and C4 come from
  the datasheet's procedure with an assumed 22 µF of effective output
  capacitance. An X5R 0805 loses a good part of that at 3.3 V of bias.
- **The valve FETs are rated below the clamp.** `IRLML6244` is a 20 V part
  (LCSC lists it as 20 V N-channel). With a valve off, its drain sits at the
  12 V rail, and a surge that the SMAJ15A clamps at 24.4 V puts 24.4 V across
  it. A 12 V rail leaves little margin on a 20 V FET to begin with. Either a
  30 V FET or a lower-clamp TVS is needed; this has not been changed here.
- **Parts operated outside the envelope their registry entries declare.**
  `jst_xh_2pin`, `p_mosfet_si2301` and `d_zener_5v1` each declare 5 V and
  100 mA, a template default and not a datasheet figure; the real devices are
  fine. Filed as #275. Until it is corrected, the current
  capability of this board's 12 V path cannot be checked by anything but a
  reader.
- **Part numbers are chosen, not purchased.** Every part the supply check
  covers now names a manufacturer part number, and all but the APDS-9301 name an
  LCSC number that JLCPCB also uses. Generic resistors and capacitors are
  specified by value and are exempt from that check. They come from LCSC and JLCPCB listings and
  were not checked against a stock or price page, so confirm the exact suffix
  before ordering. The APDS-9301 has a manufacturer number and no LCSC one:
  LCSC lists the APDS-9300-020, which is a different device. The fuse and the
  inductor are per-value parts (`polyfuse_1r1a`, `l_1210_10uh`) because the
  generic ones could not honestly carry a single product.
- **The inductor is rated, not proven.** The CBC3225T100MR is listed at 900 mA
  rated current. That is not a saturation current, and the buck's load has not
  been bounded against it.
- **The analogue path is unverified.** `stm32g030` declares no `analog_input`
  capability, so PA1 and PA2 are ordinary GPIO to validation. Reference,
  source impedance and settling against the 10 kΩ filter are a reviewer's job.
- **No thermal analysis:** the buck and both FETs switching inductive loads
  each warrant one.
- **The schematic still has the junction warning from review**
  (`E-SYNTH-SCHEM-005`: a node at (109.2, 15.2) mm carries four wire lines
  against a limit of three), plus two informational sheet-fill notes. The twelve
  decoupling placements and the overlapping region boxes are cleared. The sheet is laid out as eight bands, one
  per functional group, which is tall; the engine does not place passives
  beside their pins on its own (#207), so the pinned layout is what keeps
  them there.
- **`E-SYNTH-POWER-004`** reports `V12` with twelve load endpoints against
  three decoupling capacitors.
- `E-SYNTH-SCHEM-010` is no longer reported, because the rails are now named.
- Divider ratio and the 4k7 pull-up value are datasheet and convention, not
  measurement.
