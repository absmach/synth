# iCE40 HDMI video board

A Lattice iCE40 HX4K in a 121-ball BGA driving an HDMI Type-A output, with its
configuration flash, a 25 MHz oscillator, and 1.2 V and 3.3 V rails carried as
inner-layer planes from a single 5 V USB-C input.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 89.34 × 63.04 mm |
| Layers | 4 |
| Process | JLCPCB filled-via (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 49 |
| Source | `ice40_hdmi_video.synth` |
| Sidecars | `ice40_hdmi_video.placement.layout.toml` (all 49 pinned) |

## Stackup

| Layer | Thickness | Carries |
| --- | --- | --- |
| F.Cu | 0.035 mm | signal, ground pour |
| prepreg | 0.2104 mm, εr 4.4 | — |
| In1.Cu | 0.0152 mm | 1.2 V core plane |
| core | 1.065 mm, εr 4.6 | — |
| In2.Cu | 0.0152 mm | 3.3 V I/O plane |
| prepreg | 0.2104 mm, εr 4.4 | — |
| B.Cu | 0.035 mm | signal, ground pour |

Total 1.586 mm. The 0.2104 mm prepreg under the signal layer is what puts a
0.2 mm trace near 50 Ω single-ended over the plane below it, which is the 100 Ω
differential the TMDS pairs want.

## Supply

| Rail | Source | Nominal | Carried as | Feeds |
| --- | --- | --- | --- | --- |
| 5 V in | J1 USB-C, CC1/CC2 pulled down 5.1 kΩ | 5 V | traces | U2 and U3 inputs, J2 pin 18 |
| 1V2 core | U2 MP2307 buck, L1 4u7, 3k01/10k divider | 1.2 V | **In1.Cu plane** | U1 VCC ×4, VCCPLL0/1 |
| 3V3 I/O | U3 AMS1117-3.3 | 3.3 V | **In2.Cu plane** | U1 VCCIO ×5, VCC_SPI, VPP_2V5, VPP_FAST, U4, Y1, pull-ups |

Each rail pad reaches its plane through a via in the pad. On this process that
is forced, not chosen — see *Escape arithmetic*.

## Connectors

All pinouts below are derived from the source, not transcribed.

**J1 — USB-C receptacle, power only**

| Pin | Signal |
| --- | --- |
| VBUS ×4 (A4/B4/A9/B9) | +5 V in |
| GND ×4 (A1/B1/A12/B12), SHLD | Ground |
| CC1, CC2 | 5.1 kΩ to GND, advertising a 5 V sink |
| SBU1, SBU2 | To D1 TVS channels 3 and 4 |

D+/D− are unused: this is a power-only inlet.

**J2 — HDMI Type-A receptacle**

| Pin | Signal |
| --- | --- |
| 1, 3 | TMDS data 2 ± (via R8/R9, 270 Ω) |
| 4, 6 | TMDS data 1 ± (via R10/R11) |
| 7, 9 | TMDS data 0 ± (via R12/R13) |
| 10, 12 | TMDS clock ± (via R14/R15) |
| 2, 5, 8, 11 | Lane shields, to ground |
| 15, 16 | DDC SCL/SDA, pulled up 47 kΩ to 3V3 |
| 17 | Ground |
| 18 | +5 V to the sink, decoupled by C16 |
| 19 | Hot-plug detect, to U1 ball H7 |

## FPGA ball map

ICE40HX4K-BG121, 121 balls on an 0.8 mm grid. Seventeen signal balls are used.

| Ball | Signal |
| --- | --- |
| A1, A2 | TMDS data 2 +/− |
| A3, A4 | TMDS data 1 +/− |
| A5, A6 | TMDS data 0 +/− |
| A7, A8 | TMDS clock +/− |
| B6 | 25 MHz global clock in, from Y1 |
| L10, K10 | Flash SCK, CS |
| J9, K9 | Flash SDI, SDO |
| L8, H9 | DDC SCL, SDA |
| H7 | HDMI hot-plug detect |
| L9 | CRESET, pulled up 10 kΩ |
| K8 | CDONE, pulled up 10 kΩ, drives D2 |

The four TMDS pairs sit on the top row of the grid, which is the one row that
escapes without a dog-bone.

Power and ground: 1.2 V on D4, D8, H4, H8 and PLL supplies J6, C6; 3.3 V on
VCCIO D6, F8, H6, E4, G4, VCC_SPI L11, VPP_2V5 C10, VPP_FAST B10; ground on ten
balls plus both PLL grounds.

## Netclasses

| Class | Track | Clearance | Other | Applies to |
| --- | --- | --- | --- | --- |
| TMDS | 0.2 mm | 0.2 mm | — | the 8 lane nets |
| CORE | 0.6 mm | 0.25 mm | `plane` | the 1.2 V net |
| IO | 0.4 mm | 0.2 mm | `plane` | the 3.3 V net |

A `netclass` block assigns no nets. Each of these is joined by `class "NAME"` on
a `connect`. Without it a class is inert and reads as satisfied.

## Differential pairs

Four pairs, each `impedance 100ohm`, `max_skew 0.1mm`, `couple tight`, declared
on the connector side of the 270 Ω series resistors where the controlled run is.

`max_skew` is checked, not merely recorded: `E-SYNTH-DRC-014` sums each half of
the pair from the emitted segments and fails one whose halves differ by more
than the budget, whatever the router reported about its own matching. `couple`
is not checked — nothing keeps the two halves of a pair together.

The iCE40 has no dedicated TMDS transmitter, so the pairs are driven as
pseudo-differential LVCMOS33 from one bank with series termination, which is how
the open-source iCE40 HDMI designs do it. That is good for 640×480 and not for
1080p; a real 1080p board uses a transmitter such as the TFP410, and the
registry has no such part.

## Escape arithmetic

A property of the process, not of the router. At JLCPCB's standard floor a via
is 0.3 mm drill plus 2 × 0.13 mm annular, so 0.56 mm across. Dropped between two
balls 0.8 mm apart it sits 0.400 mm from each ball centre and needs 0.582 mm to
clear the pad copper. Dog-bone escape is therefore impossible on this tier, and
via-in-pad is the required technique rather than a shortcut.

The board declares that in its own source. `requires via_in_pad` states the
process it cannot be built without, and `manufacturer "jlc-filled-via"` names a
tier that offers it; the two have to agree, because a board cannot approve its
own fabrication, and declaring the requirement against a tier that does not
fill is an error rather than a warning.

U1 also declares *how* it escapes — `escape { method underpad layers "In1.Cu",
"In2.Cu" }` — because the method is a design decision even where the geometry
would have selected the package anyway.

What the run does: four packages are handed to the escape engine before the
general router sees the board — U1, U2 and the two connectors J1 and J2, each
selected by pad geometry rather than by name. Nine escape vias are placed at
0.56 mm on a 0.3 mm drill, which is `min_drill + 2 x min_annular` for this
tier. Fourteen of the plane's stitching vias go in their pads, which is what
`requires via_in_pad` is for: those are interior supply balls with a neighbour
a pitch away on every side and no room beside them.

## Notable parts

| Ref | Part | Registry id |
| --- | --- | --- |
| U1 | ICE40HX4K-BG121 | `ice40hx4k_bg121` |
| U2 | MP2307 | `mp2307_buck` |
| U3 | AMS1117-3.3 | `ams1117_3v3` |
| U4 | W25Q128JV | `w25q128_flash` |
| Y1 | 25 MHz oscillator | `osc_smd_25mhz` |
| J1 | USB-C receptacle | `usb_c_receptacle` |
| J2 | HDMI Type-A receptacle | `hdmi_a_receptacle` |
| D1 | TVS array SOT-363 | `tvs_array_5v` |

`ice40hx4k_bg121` is the registry's first BGA part and `hdmi_a_receptacle` its
first HDMI connector; both were written for this board and verified ball by ball
against the stock KiCad symbol and footprint. The generated `bom.csv` is the
bill of materials; this table is the actives and omits 39 generic passives.

## Limitations

- **The board does not route completely.** 27 nets closed, 14 open, 949.9 mm of
  copper. `net_8` is the worst: `U1.C6` reaches 6 of its 14 pads. That is
  signal routing a human would finish by hand.
- **The TMDS lanes carry no lane-to-lane length budget.** Each pair bounds its
  own two halves with `max_skew`, which is declared and checked. A budget
  across the four pairs would be a group tolerance on the class, and the
  language has no syntax that reaches the IR for one, so the lanes are matched
  against each other by nothing.
- **`kicad-cli pcb drc` reports 288 errors, 14 unconnected pads and 35
  warnings** on the routed copper: 198 clearance, 39 shorting items, 28 hole
  clearance, 16 via diameter, 14 dangling vias, 10 dangling tracks, and the
  rest mask bridges and hole-to-hole. A 121-ball grid on four layers is more
  than this engine completes cleanly. No fabrication artifacts are produced.
- The 16 via-diameter errors are Synth disagreeing with itself rather than the
  engine failing. The escape vias are sized from the process floor at 0.56 mm,
  while every exported net class states 0.6 mm with a 0.4 mm drill — a 0.10 mm
  annular ring, under the 0.13 mm minimum the same project file declares.
- **The fabrication floor is respected.** No track is below 0.127 mm and no
  via below 0.3 mm drill or 0.13 mm ring, which is new: the exported project
  previously stated neither the drill nor the ring minimum, and the engine
  took its own 0.2 mm.
- **Impedance is not verified on the inner layers.** Only outer-layer microstrip
  is derived from the stackup, so the four `W-SYNTH-IMPEDANCE-001` warnings say
  the pairs may be routed where impedance is not checked. A pair cannot be
  pinned to a layer at all.
- Return paths, layer transitions, via stubs, neck-downs and plane continuity
  are not analysed. The compiler's capability descriptor lists all of them as
  unsupported.
- No thermal analysis and no power sequencing: enable ordering and power-good
  are neither declared nor checked.
- `E-SYNTH-POWER-004` reports the 5 V net carrying 11 load endpoints against 2
  decoupling capacitors. Fair on a board with this many supplies.
- Nine `E-SYNTH-SCHEM-003` warnings quote schematic separations that depend on
  KiCad symbol geometry; read them from CI, not from a local run.
- Four parts carry no distributor identity, so the BOM is not orderable as it
  stands.

## Reproducibility

The sidecar pins all 49 components, and three consecutive exports produce a
byte-identical board.

Two of those coordinates describe the request rather than the result. Edge
legalisation runs after a `hard` override and moves J1 by −0.56 mm in y and J2
by +0.725 mm in x — the same step every run, whatever they are pinned to
. Because the offset is constant the board stays deterministic, which is
what rerun stability needs; what it costs is that the sidecar is not a literal
description of where those two connectors end up. A true fixed point, where the
pinned value is reproduced verbatim, is not reachable for them while the offset
is non-zero.

Routing is deterministic for a given board name: the name seeds the entity
uuids, so renaming changes the order the router works in. Compare connectivity,
not via counts.
