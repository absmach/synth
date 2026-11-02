# Qualification record: PT100 4-wire RTD analog front end (SYN-363)

This file records what was measured on the exported board. It is not a
qualification sign-off. `reports/release.json`'s `release_ready` and
`production_status` cover only Synth's router, DRC and signal-integrity gates;
its own `gate_scope` says so. Every review gate below that needs a human is
still open.

The board is not fabricated, not powered and not measured. The SI/PI/RF review
is in progress with the designated reviewer; do not call it proven, clean or
production-ready.

## Board and stackup

- Outline: 50.1 x 44.1 mm bounding box, 1880 mm² after the top-left notch.
  The closed path is in `rtd_afe.placement.layout.toml` as `[[outline]]`; the
  exporter uses it instead of the fitted rectangle. Edge.Cuts runs x 2.50 to
  52.59 mm, y -0.01 to 44.08 mm, with the rectangle x 10.0 to 26.4 mm,
  y -0.01 to 20.0 mm cut away. The left tab (x 2.5 to 10 mm) keeps H1 and the
  top-left ground fence via.
- Four copper layers with an explicit 1.5862 mm FR4 stackup: 0.035 mm copper,
  0.2104 mm prepreg (er 4.4), 0.0152 mm copper, 1.065 mm core (er 4.6),
  0.0152 mm copper, 0.2104 mm prepreg (er 4.4), 0.035 mm copper. No
  controlled-impedance stackup was requested.
- One `GND` zone on each of the four copper layers. There is no AGND/DGND split
  and no tie resistor. The four mounting holes have 5.6 x 5.6 mm all-layer rule
  areas around them; the notch removes the empty top-left pour, not any pad.
- 53 footprints, 40 nets. The analog chain (J2, the RC filter, U1, U3, L2) is
  on the left and bottom-left; the MCU, flash, crystal, USB-C and LDO are on the
  right and top-right.

### Mechanical geometry

Measured on `kicad/rtd_afe.kicad_pcb` from the board edge and the footprint
bounding boxes. These are drawing facts, not a fit check: no enclosure model,
screw head, cable or mating connector was measured.

| Item | Measured | Status |
| --- | --- | --- |
| Board | 50.1 x 44.1 mm bounding box; notch x 10.0..26.4, y 0..20.0; left tab 7.5 mm wide | from `[[outline]]` |
| H1 | centre (6, 4); 3.50 mm from left edge, 4.01 from top, 4.00 from notch edge; 2.25/2.76/2.75 mm of material | tab strength not checked |
| H2 | centre (49, 4); 3.59 mm from right edge, 4.01 from top; 2.34/2.76 mm of material | not checked |
| H3 | centre (6, 40); 3.50 mm from left edge, 4.08 from bottom; 2.25/2.83 mm of material | not checked |
| H4 | centre (49, 40); 3.59 mm from right edge, 4.08 from bottom; 2.34/2.83 mm of material | not checked |
| J1 | body bbox x 34.16..44.84, y 1.26..10.73; 1.27 mm from top edge; centred on the main top edge (39.5 against 39.495) | overhang/enclosure not checked |
| J2 | body bbox x 9.89..21.11, y 39.70..43.30; 0.78 mm from bottom edge | mating fit not checked |
| J3 | body bbox x 37.16..45.84, y 39.70..43.30; 0.78 mm from bottom edge | mating fit not checked |

The mounting holes are non-plated, 2.5 mm, with 5.6 x 5.6 mm all-layer rule
areas. The tab carries H1 and the top-left ground fence via; it is the only
7.5 mm-wide connection between the left strip and the rest of the board above
y = 20 mm, so its strength and the screw-head clearance are open mechanical
items.

## Check results

Commands, from the repository root (`check.sh` does all of this):

```
export XDG_DATA_HOME=$(mktemp -d)
target/debug/synth export-kicad qualification/class-b/rtd-afe/rtd_afe.synth \
  --out qualification/class-b/rtd-afe/kicad --router free-routing \
  --allow-unverified-parts --validate-erc --pnp --gerbers --drill
kicad-cli pcb drc --severity-all --refill-zones \
  --format json -o qualification/class-b/rtd-afe/reports/rtd_afe.kicad-drc.json \
  qualification/class-b/rtd-afe/kicad/rtd_afe.kicad_pcb
kicad-cli sch erc --severity-all --format json \
  -o qualification/class-b/rtd-afe/reports/rtd_afe.kicad-erc.json \
  qualification/class-b/rtd-afe/kicad/rtd_afe.kicad_sch
```

Versions: KiCad 10.0.7, FreeRouting 2.4.1 on OpenJDK 25.0.4.1, Synth
`synth-cli 0.1.1`.

| Check | Result | Evidence |
| --- | --- | --- |
| KiCad DRC, all severities | 0 errors, 0 warnings, 0 unconnected items | `reports/rtd_afe.kicad-drc.json` (`violations: []`, `unconnected_items: []`) |
| Schematic/PCB parity | empty | same DRC report |
| pcbnew reopen | OK | `check.sh` loads the board with `pcbnew.LoadBoard` |
| Ground | one `GND` plane; `AGND` and `DGND` are absent | `check.sh` asserts it; see below |
| KiCad ERC | 0 errors, 2 warnings | `reports/rtd_afe.kicad-erc.json` |
| Synth routing | routed, 40 connected nets, 0 open | `reports/rtd_afe.routing.json` |
| Synth release | `production_status: untrusted`, `release_ready: false` | `reports/release.json`; the unverified-crystal override, not a DRC failure |
| Synth signal integrity | 19 warnings, 0 blocking | `reports/release.json` |

The delivered board is `kicad/rtd_afe.kicad_pcb`
(`3a98c4656d43e61a4c81889b03e74d61eb20fabad1d08d2af27b2598499dda74`).
The schematic is `kicad/rtd_afe.kicad_sch`
(`c7300d22238d8d4991b3ba453599ffdb96a833de3c83bdc132542ea662537812`).

### DRC history

The earlier 58 x 47, 60 x 49, 64 x 49 and 67 x 49 mm attempts failed on Synth's
ground-island repair: the split pours fragmented around U2. The compact rework
grouped the analog chain at the left, the MCU/flash/crystal and USB at the
right, and put J1/J2/J3 on the edges. On the merged toolchain the split-ground
version of this 50.1 x 44.1 mm board still left two DGND pour islands near
(32.55, 32.75) and (41.45, 24.03) mm; no 0.6 mm tie via fitted. Replacing the
split with one GND plane removed the islands. The route then closed after three
placement moves that opened tight pockets: C_DV2 to (44.5, 26.5), L1 to
(43.5, 20.0) and C_VI to (45.0, 24.2), with the related R_RST/C_DV and L2
positions adjusted at the same time. The delivered board has 0 errors, 0
warnings and 0 unconnected items.

A 12-test-point variant was then placed on the same outline: VREF, GND, +3V3A,
VBUS, +3V3, +3V3_RP_A, DVDD_1V1, RUN, USB_DP, USB_DN, SWDIO and SWCLK. Its
route failed on a TP6 edge clearance, a QSPI_SS/+3V3 clearance at U4 and two GND
pour islands, so the test points are not in the delivered board. Fitting the
full set needs a larger outline or a different placement; see the README.

The branch placer exposed a sidecar trap: a floorplan whose entries are all
absolute is passed through `close_pinned_bands`, which slides the pinned parts
to shut gaps. It moved U4 onto U2 and produced 91 mask/clearance violations.
Keeping one relative entry (`C_FL` relative to `U4`), as the original sidecar
did, disables that pass; the floorplan then exported exactly as written. This is
recorded under Synth defects below.

The first clean route still had 56 silkscreen warnings because reference labels
sat on neighbouring pads. U1's label was moved to (24, 34.5) mm; 25 passives/TVS
and H1/H2/H3/J1 have their reference label hidden; the delivered board has 0
silk warnings.

### The compact rework

This rework is 50.1 x 44.1 mm with a 16.4 x 20 mm notch, 1880 mm², and routes
with 0 open nets, 0 DRC errors, 0 warnings and 0 unconnected items. That does
not prove it is the densest arrangement; it is one successful FreeRouting run,
and `check.sh` re-routes from scratch.

J2 and J3 are level at y = 41.5 mm, with their bodies 0.8 mm from the bottom
edge. J1 is at (39.5, 6.0) mm on the main top edge, which runs x 26.4..52.59 mm
after the notch, so J1 is centred on that edge. Centering J1 on the full
rectangle (x = 27.5) was tried first; it left an unconnected ground pour sliver
at U2's 0.4 mm pad row, and the stitching search could not tie it. The notch
removes the empty top-left corner instead, keeping the proven route and making J1
the centre of the remaining top edge. The outline is a closed `[[outline]]` path
in the placement sidecar; the exporter uses it, so a normal `synth export-kicad`
reproduces the notch without a post-export cut.

## ERC findings and dispositions

`reports/rtd_afe.kicad-erc.json` has two warnings and no errors:

| Warning | Disposition |
| --- | --- |
| `lib_symbol_issues`: Symbol `Ferrite_Bead` not found in `Device` | The registry part uses `Device:Ferrite_Bead`; KiCad 10's library has `Device:FerriteBead`. The schematic embeds its own symbol and the netlist is correct. The registry data is stale; not changed here to keep this change set to this board. |
| `lib_symbol_mismatch`: Symbol `REF5025ID` does not match the copy in `Reference_Voltage` | The embedded symbol is generated from the registry pins, so its drawing differs from the installed library copy. Pin numbers and electrical types match the reviewed part; the netlist and PCB parity are clean. Cosmetic. |

Both are library-comparison warnings from an embedded-symbol export, not
connectivity findings. A human should confirm that disposition.

## Synth signal-integrity warnings

`reports/release.json` lists 19 non-blocking signal-integrity findings: 17 are
layer changes with no ground-return via within 0.64 mm, and 2 are crosstalk
pairs. The layer changes are on REFOUT, SPI_MOSI/CS/SCK/MISO, ADC_DRDY, XIN, RUN,
SWCLK, USB_DP_C and USB_DN_C. The crosstalk pairs are SWDIO/VBUS (2.7 mm
parallel, 0.156 mm apart) and QSPI_SS/QSPI_SD2 (2.9 mm, 0.194 mm apart). No
RTD_FP/RTD_SP/RTD_SN/RTD_FN, AIN1, AIN2 or VREF finding appears. The six
RTD/input nets and VREF are on F.Cu only; QSPI also stays on F.Cu.

The XIN and USB connector-side layer changes are the ones an SI reviewer should
look at first: XIN is a high-impedance crystal net and the USB pair changes
layers on both sides. The USB path is also not length-matched: the MCU-side and
connector-side lengths add to 24.13 mm on DP and 19.40 mm on DN, a 4.73 mm
difference. A return via next to each layer change would help; this was not
added, because it is a layout change that needs an SI review first.

## Synth validate warnings

`target/debug/synth validate` (with an empty `XDG_DATA_HOME`) also emits
warnings. They are not part of the DRC acceptance bar, but they are not hidden:

| Finding | Disposition |
| --- | --- |
| `E-SYNTH-POWER-004` on VBUS: 7 load endpoints, 1 capacitor | Heuristic. C_L1 (2.2 uF) is the LDO input capacitor. There is no separate VBUS bulk capacitor, fuse or TVS; that is a real gap, see the USB section. |
| `W-SYNTH-ANOMALY-001` | Statistical topology heuristic (score -2.43 against a 0 boundary); no action. |
| `W-SYNTH-SUPPLY-002` x10 | Generic parts (passives, headers, ESD) have no distributor identity. |
| `W-SYNTH-PART-UNVERIFIED` | Y1, the 12 MHz crystal, has no part-level reviewer. This is the release override. |
| `E-SYNTH-CONNECT-007` x2 | `SPI_MISO` and `ADC_DRDY` join a `bidirectional` MCU pin to an ADC `output`. Normal SPI/ready wiring; the MCU pin is only an input in those modes. |
| `E-SYNTH-ESD-001` x3 | VBUS has no reverse-polarity protection and CC1/CC2 have no TVS. Real gaps; see USB. |
| `E-SYNTH-SCHEM-003` x11 | Decoupling-cap symbol bodies are more than 25 mm from their IC bodies in the compiler-drawn schematic. A schematic-drawing warning, not the PCB. |
| `E-SYNTH-SCHEM-004`, `E-SYNTH-SCHEM-005` | One 119.4 mm drawn wire and one 4-way junction in the compiler-drawn schematic. Drawing only. |

## Per-part datasheet review

"Reviewed" below means the registry's `provenance.reviewed_by` is set by Steve
Munene (2026-10-27) and the datasheet pin tests
(`rtd_front_end_parts_match_their_datasheet_pinouts` for U1/U3,
`ble_node_parts_match_their_datasheet_pinouts` for U5) check the listed key
pins. It does not mean this work re-read the whole datasheet or that a second
person checked it. "Not reviewed" is the honest state of every other part.

| Ref | Part / MPN | Datasheet | Pins checked | Decoupling / support | Review |
| --- | --- | --- | --- | --- | --- |
| U1 | TI ADS124S08IRHBR (RHB VQFN-32) | TI SBAS660C, https://www.ti.com/lit/ds/symlink/ads124s08.pdf | AINCOM 1, AIN0 7, CS 9, DOUT/DRDY 12, DGND 14, DVDD 16, CLK 17, REFOUT 23, NC 25, AVDD 26, AVSS 27, REFP1/AIN6 32, EP 33; 13 of 33 checked | C_AV1 330 nF + C_AV2 10 uF on AVDD (+3V3A); C_DV 100 nF DVDD; C_IO 100 nF IOVDD; C_RO 1 uF REFOUT | Reviewed, registry `ads124s08` |
| U3 | TI REF5025AIDR (SOIC-8) | TI SBOS410I, https://www.ti.com/lit/ds/gpn/ref5025 | VIN 2, TEMP 3, GND 4, VOUT 6, NC 7; 5 of 8 | C_RI 1 uF VIN, C_RV 1 uF VOUT | Reviewed, registry `ref5025` |
| U2 | Raspberry Pi RP2350A (QFN-60) | https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf (from the KiCad symbol; not opened in this work) | Pin numbers compared against the installed KiCad `MCU_RaspberryPi:RP2350A` symbol: GPIO0 2, GPIO1 3, GPIO2 4, GPIO3 5, USB_DM 51, USB_DP 52, VREG_AVDD 46, VREG_PGND 47, VREG_LX 48, VREG_VIN 49, VREG_FB 50, IOVDD 1/11/20/30/38/45, DVDD 6/23/39. The registry's own note claiming RP2040 numbering is stale | C_IO1..C_IO4 100 nF on IOVDD; C_DV1/C_DV2 100 nF + C_DVB 4.7 uF on DVDD 1.1 V; C_VI 4.7 uF VREG_VIN; R_AV 33R + C_AVF 4.7 uF on VREG_AVDD/ADC_AVDD; L1 3.3 uH on VREG_LX | Not reviewed |
| U4 | Winbond W25Q128JVSIQ (SOIC-8) | None in the registry | CS 1, IO1 2, IO2 3, GND 4, IO0 5, CLK 6, IO3 7, VCC 8; all 8 used | C_FL 100 nF | Not reviewed |
| U5 | TI LP5907MFX-3.3 (SOT-23-5) | TI SNVS798Q, https://www.ti.com/lit/ds/symlink/lp5907.pdf | VIN 1, GND 2, EN 3, VOUT 5; pin 4 modelled NC/open | C_L1 2.2 uF VIN, C_L2 2.2 uF VOUT | Reviewed, registry `lp5907_3v3` |
| J1 | HRO TYPE-C-31-M-12 (USB-C) | None in the registry; LCSC C165948 | 16 signal/power pins plus 4 shield pads: A1/A12/B1/B12/SH GND, A4/A9/B4/B9 VBUS, A5 CC1, B5 CC2, A6/B6 DP, A7/B7 DN; SBU1/SBU2 open | 5.1k CC1/CC2; 27R DP/DN; D1/D2 TVS | Not reviewed |
| D1, D2 | `esd_usb` generic 2-pin TVS (SOD-323) | None; no MPN | IO to DP/DN, GND | — | Placeholder part, not reviewed |
| Y1 | ECS-120-20-33-TR, 12 MHz, CL 20 pF | https://ecsxtal.com/store/pdf/ecx-32.pdf | Pads 1/3 crystal, 2/4 ground, per the registry description; not re-read in this work | C_X1/C_X2 33 pF C0G; R_X 1k on XOUT | Not reviewed (release override) |
| L2 | Generic 0603 ferrite bead, 600 ohm at 100 MHz | None | 2 pins | — | Generic |
| L1 | Generic 0805 3.3 uH | None | 2 pins | — | Generic |
| R_S1/R_S2, R_BIAS, R_AV, R_RST, R_RUN, R_X, R_CC1/2, R_DP/DN | Generic 0603 | None | 2 pins | Values in the BOM; R_S1/R_S2 and R_BIAS are 0.1 % in the source | Generic |
| C_* | Generic 0603/0805 | None | 2 pins | C0G for the filter and crystal; X7R for the supply decoupling | Generic |
| J2, J3 | Generic 2.54 mm headers (1x4, 1x3) | None | 4 and 3 pins | — | Generic |
| H1..H4 | Generic M2.5 NPTH | None | 1 hole | — | Generic |

The registry's required decoupling is met: ADS124S08 AVDD 330 nF and DVDD
100 nF; REF5025 VIN/VOUT 1 uF; LP5907 VIN/VOUT 1 uF (2.2 uF fitted); RP2350
IOVDD 4 x 100 nF and DVDD 2 x 100 nF; W25Q128 VCC 100 nF. Proximity of each
support part to the pin it serves was measured on the earlier split-ground
route; the single-ground rework moved C_DV2, C_VI and L1, so those distances
are not carried forward as verified numbers.

The BOM (`kicad/bom.csv`) now carries `dielectric`, `voltage`, `tolerance` and
`power` columns copied from the structured attributes in `rtd_afe.synth`, so the
C0G filter capacitors and the 0.1 % resistors are visible to an assembler that
reads only the BOM. Empty cells mean the source did not declare that attribute.

## Single ground plane

The design requirement is one ground reference for the analog and digital
returns. The delivered board satisfies it as follows:

1. `GND` is the only ground net. Four zone objects, one per copper layer, each
   carry it. There is no `AGND` or `DGND` net, zone or tie part.
2. Every track and via carries one net. A zone fill belongs to one zone and
   therefore one net.
3. `check.sh` loads the board in pcbnew and asserts that `GND` exists and that
   `AGND` and `DGND` do not. On the delivered board it prints
   `reopen ok; single GND plane`.

Why one plane: the ADC's digital return (DGND pin 14), CLK, AVSS, AVSS_SW,
REFN0, REFCOM and exposed pad return to the same plane as the MCU, flash, USB
and LDO, so a signal that crosses the analog/digital boundary never has to
change reference planes. The earlier split version needed a tie and still left
two isolated pours on the merged router. No impedance or ground-noise
measurement was made; this is the main item for the analog/SI review.

## Measured layout numbers

- 384 track segments, 61 vias: 41 GND, 20 signal. 590.5 mm of track. 40 nets, 0
  open. The router records 58 vias; the other three are source ground
  stitching.
- Analog nets, all F.Cu only, no via:

| Net | Length |
| --- | --- |
| RTD_FP | 11.02 mm |
| RTD_SP | 4.17 mm |
| AIN1 | 9.23 mm |
| RTD_SN | 4.17 mm |
| AIN2 | 14.06 mm |
| RTD_FN | 8.32 mm |
| VREF | 4.83 mm |

- SPI_SCK is 16.79 mm; SPI_MOSI/MISO/CS are 14.62/17.44/16.65 mm. All four SPI
  nets change layer (two vias each). QSPI_SS/SCLK/SD0..SD3 are
  11.87/4.80/3.23/10.26/7.39/6.22 mm and stay on F.Cu.
- USB: the RP2350-side nets are 12.71 mm (DP) and 10.83 mm (DN); the
  connector-side nets are 11.42 mm and 8.57 mm. Total path 24.13 mm (DP) and
  19.40 mm (DN), 4.73 mm of mismatch, full speed. Both sides change layers; the
  pair is not routed as a coupled pair and has no controlled impedance.
- Crystal: XIN 12.57 mm, XOUT 14.21 mm, XOUT_R 8.38 mm. XIN changes layer; that
  is one of the SI warnings above.
- 29 reference labels are hidden: C_AV1, C_AVF, C_CM1, C_CM2, C_D, C_DV, C_DV1,
  C_DV2, C_IO, C_IO1, C_IO2, C_IO3, C_IO4, C_RV, C_VI, D1, D2, H1, H2, H3, J1,
  R_AV, R_BIAS, R_CC2, R_DN, R_DP, R_S1, R_S2, R_X. They remain in the BOM, PnP
  and schematic.
- Board outline and stackup above; 53 footprints.

## Left to a human

Nothing below is signed off. The list is deliberately blunt.

- **SI/PI/RF review (designated reviewer)**: the 24-bit analog layout, the
  single ground plane, the ADC reference decoupling, the RC filter against the
  chosen ADC data rate, the IDAC output's 10 nF load and settling, the PGA
  common-mode range at the chosen gain, the unused-input termination, the 17
  layer-change warnings, the two crosstalk pairs, and the 4.73 mm USB length
  mismatch. None of these were simulated or measured.
- **Metrology/analog**: the measurement is not ratiometric. The IDAC's absolute
  accuracy and drift, the REF5025 grade and drift, the 1 k filter resistors and
  the PT100 wiring resistance set the accuracy; there is no calibration or
  error budget. A 4-wire measurement with 0.1 % resistors is not automatically a
  0.1 % thermometer.
- **USB/ESD**: no VBUS fuse, TVS or reverse-polarity protection; CC1/CC2 have no
  TVS; the shield is tied directly to GND; D1/D2 have no MPN. USB-C and ESD
  compliance were not checked.
- **Mechanical**: the measured margins are in the table above. In short, the
  mounting holes leave 2.25 to 2.83 mm of material around a 2.5 mm hole, the
  bottom connectors are 0.78 mm from the edge, J1 is 1.27 mm from the top edge,
  and H1 sits on the 7.5 mm tab 2.75 mm from the notch edge. Tab strength, the
  enclosure, screw-head clearance, connector overhang and header orientation
  were not checked. Synth places no board-name or revision silk.
- **Test points**: none are fitted. The 12-pad set was tried and did not route
  on this outline; a bench bring-up therefore has to probe component pads or a
  harness. A larger board or a reduced set is a design decision, not an
  exporter limitation.
- **Fabrication/DFM**: no fab house has seen the Gerbers, drill, BOM or PnP.
  The 0.4 mm QFN-60, the 0.5 mm VQFN-32 and the 0.5 mm USB-C need a fab
  capability check; the four-layer stackup is declared by the source, not a
  controlled-impedance quote. Assembly of the generic passives needs the
  C0G/tolerance attributes from the source, not just the BOM values.
- **Firmware**: the RP2350 boot, QSPI flash, ADC gain/data-rate/IDAC registers,
  USB descriptors and the PT100 conversion/calibration do not exist in this
  repository. Nothing was powered.
- **Independent sign-off**: the board review is with the designated reviewer.
  Part-level reviews are only the three registry entries in the table (U1, U3,
  U5); Y1 still has none. There has been no second-person check of this record
  and no hardware measurement. The only automated checks are the datasheet pin
  tests, the KiCad DRC/ERC and the ground assertion.

## Synth defects hit in this work

1. **Duplicate footprint-body UUIDs (fixed).** Inlining a KiCad 10 footprint
   body kept the library's child UUIDs, and `ensure_body_uuids` only filled
   missing ones and keyed them by `lib_id`, not by the placed instance. Every
   0603 shared its pads' and silk lines' UUIDs: 79 duplicated ids, one shared by
   23 footprints. KiCad keys board items by UUID, so `kicad-cli pcb drc`
   attributed a silk collision to an item 11 mm away (R_AV's label against
   C_L2's silk). The fix gives each instance its own deterministic ids and
   replaces stock ones; the regression test is
   `inlined_body_blocks_carry_unique_deterministic_uuids`. After the fix every
   board item carries its own id, so the DRC warnings name the real neighbours.
   This is what made the zero-warning result trustworthy.
2. **Ground stitching is placement-sensitive (not fixed).** Earlier attempts
   left split-ground islands that the 0.6 mm stitching-via search could not tie,
   so the export refused them. The single-plane version has no islands, but a
   future placement change can bring them back; `check.sh` catches it. The
   return-via half of this repair is now advisory (see item 7).
3. **An all-absolute placement sidecar is silently re-packed (not fixed).**
   With every entry absolute, the export's `close_pinned_bands` pass slid the
   pinned parts to shut gaps; it moved U4 onto U2 and produced 91 mask and
   clearance violations while the sidecar looked as if it had been ignored.
   Keeping one relative entry (`C_FL` relative to `U4`), as the original
   sidecar did, disables the pass. The export should either preserve an
   absolute floorplan or refuse it, not re-pack it into an illegal one.
4. **BOM omitted component attributes (fixed).** `bom.csv` had value, kind,
   symbol, footprint, LCSC and MPN, but not `dielectric`, `voltage` or
   `tolerance`, which are present in the `.synth` source. The exporter now
   writes `dielectric`, `voltage`, `tolerance` and `power` columns from the
   component properties; the regression test is
   `structured_attributes_get_their_own_columns`.
5. **No free-text silkscreen (not fixed).** The board name, revision and header
   pin functions cannot be put on the silk; only reference labels exist.
6. **The board outline lives in the placement sidecar, not the `.synth` source
   (not fixed).** The notch is an `[[outline]]` path in
   `rtd_afe.placement.layout.toml`, consumed by the normal exporter; a plain
   `synth export-kicad` reproduces it. A `.synth` outline statement would keep
   the geometry in the same file as the netlist, but the sidecar is part of the
   committed source pair and no post-export step is needed.
7. **Return-via misses blocked a whole candidate (fixed).** The island repair
   reported a signal via with no ground return as an unresolved island, so a
   route with clean connectivity could not be promoted. A missing return via is
   the existing `W-SYNTH-DRC-016` SI warning, so it is now advisory; an untied
   copper island still blocks. The regression test is
   `an_unreturned_signal_via_is_advisory_not_blocking`.
8. **Source ground fence ignored the sidecar outline (fixed).** The fence vias
   were placed against the placer's fitted rectangle, so three of them landed
   on or outside the notch edge after the custom outline was applied
   (0.0/0.26 mm copper-to-edge). The fence now follows the `[[outline]]` path and
   skips any point outside it; the delivered board has no edge-clearance error.

## Reproducibility

Inputs and outputs of the committed run:

| File | SHA-256 |
| --- | --- |
| `rtd_afe.synth` | `b7c0aebf36d67b62a749bbe78fddff50d586ddb84002a628ffe504ae7122859e` |
| `rtd_afe.placement.layout.toml` | `6e9b83c0969759a2a6df866e53553b70c3646490254362002253d70469d786e1` |
| `check.sh` | `39bd0d10444b8f55238be5b1c675b86eef36f26d1054896f1b57b286f37168d1` |
| `kicad/rtd_afe.kicad_pcb` | `3a98c4656d43e61a4c81889b03e74d61eb20fabad1d08d2af27b2598499dda74` |
| `kicad/rtd_afe.kicad_sch` | `c7300d22238d8d4991b3ba453599ffdb96a833de3c83bdc132542ea662537812` |
| `reports/rtd_afe.kicad-drc.json` | `605461330f28d51c069a59d4e70113496e40234576f247bc95c40285d99c9b61` |
| `reports/rtd_afe.kicad-erc.json` | `61826e81dcbae8011d3c7491d8df352ffa2d16de56548b602f2702186cd71674` |
| `reports/release.json` | `234170a679a98704cadcd7f7f0e98e8b9855e59b38678885dd25cebf335d050f` |
| `reports/rtd_afe.routing.json` | `a83de382445a79c178aff9795a670755bfa1d8f532cd55039db23490e9a890b6` |

`check.sh` sets `SYNTH_SOURCE_REVISION` from the checked-out commit, and both
`release.json` and the routing provenance record it. It also rewrites the
checkout path in `release.json` and `rtd_afe.routing.json` to a
repository-relative one, matching the usb-uart board's convention. The
`source_revision` names the source commit; the later artifact commit only adds
the regenerated files. All other reports are as the tools wrote them.
`SHA256SUMS` covers the rest of the deliverables.

The failed attempts described above did **not** route clean; the committed
placement did. I did not run the final inputs three times to claim a
byte-identical board, so the committed board is one successful run, not a
proven reproducible one. FreeRouting 2.4.1 and KiCad 10.0.7 on another machine
may produce a different route. `check.sh` is the repeatable acceptance check;
it re-routes from the sources and fails on any DRC error, unconnected item, or
ground violation.
