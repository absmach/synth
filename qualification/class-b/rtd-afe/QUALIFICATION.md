# Qualification record: PT100 4-wire RTD analog front end (SYN-363)

This file records what was measured on the exported board. It is not a
qualification sign-off. `reports/release.json` now says `production_status: production` and
`release_ready: true`. Those fields cover only Synth's routing and export gates
(override flags, route validation, signal integrity); its own `gate_scope` says
so. Every review gate below that needs a human is still open.

**Evidence status.** The committed `kicad/`, `reports/`, `images/` and `SHA256SUMS` are historical output from source revision 01a77d54. They are not reproducible from the current tree (see Reproducibility). `reports/release.json` (`source_revision` 01a77d54, `production_status: production`, `release_ready: true`, all reviewers `reviewed`) is generated output from that revision and does not reflect this PR's status: untrusted, `release_ready: false`. The PR stays untrusted because the committed evidence is historical and the current tree does not route, not because a part lacks a reviewer (the registry reviews, including `ecs_120_20_33_tr`, are recorded). `kicad/bom.csv` is likewise historical and predates the source sourcing pins.

The board is not fabricated, not powered and not measured. The SI/PI/RF review
is in progress with the designated reviewer; do not call it proven, clean or
production-ready.

## Board and stackup

- Outline: 40.1 x 44.2 mm rectangle, 1772 mm². A plain rectangular
  `[[outline]]` runs x 2.50 to 42.59 mm, y -0.01 to 44.21 mm; there is no notch
  and no post-export cut.
- Four copper layers; the stackup below (1.5862 mm FR4) is a design note in `agent-run/DESIGN_NOTES.md`, not declared in `rtd_afe.synth` or `rtd_afe.kicad_pro`, and the exported board has `(thickness 1.6)`: 0.035 mm copper,
  0.2104 mm prepreg (er 4.4), 0.0152 mm copper, 1.065 mm core (er 4.6),
  0.0152 mm copper, 0.2104 mm prepreg (er 4.4), 0.035 mm copper. No
  controlled-impedance stackup was requested.
- One `GND` zone on each of the four copper layers. There is no AGND/DGND split
  and no tie resistor. The four mounting holes have 5.6 x 5.6 mm all-layer rule
  areas around them.
- 53 footprints, 40 nets. The input chain (J2, the RC filter, U1) is in the
  bottom-left; the reference and analog-supply support (U3, C_RI, L2, C_RO,
  R_RST, R_CC2) fill the top-left; the MCU, flash, crystal, USB-C and LDO are on
  the right. The analog and digital groups interlock in y rather than sitting in
  two sparse columns.

### Mechanical geometry

Measured on `kicad/rtd_afe.kicad_pcb` from the board edge and the footprint
bounding boxes. These are drawing facts, not a fit check: no enclosure model,
screw head, cable or mating connector was measured.

| Item | Measured | Status |
| --- | --- | --- |
| Board | 40.1 x 44.2 mm rectangle, 1772 mm² | sidecar `[[outline]]` |
| H1 | centre (6, 3.5); 3.50 mm from left edge, 3.51 from top; 2.15/2.16 mm of material around the 2.7 mm hole | not checked |
| H2 | centre (39, 3.5); 3.59 mm from right edge, 3.51 from top; 2.24/2.16 mm of material | not checked |
| H3 | centre (6, 40.7); 3.50 mm from left edge, 3.51 from bottom; 2.15/2.16 mm of material | not checked |
| H4 | centre (39, 40.7); 3.59 mm from right edge, 3.51 from bottom; 2.24/2.16 mm of material | not checked |
| J1 | body bbox x 17.20..27.89, y 1.26..10.73; 1.27 mm from top edge; centred on the board (centre x = 22.545) | overhang/enclosure not checked |
| J2 | body bbox x 9.20..20.41, y 39.91..43.49; 0.71 mm from bottom edge | mating fit not checked |
| J3 | body bbox x 23.66..32.34, y 39.91..43.49; 0.71 mm from bottom edge | mating fit not checked |

The mounting holes are non-plated, 2.7 mm (M2.5 clearance), with 5.6 x 5.6 mm all-layer rule
areas. J1 is centred on the top edge. The enclosure and screw-head clearance
are open mechanical items.

## Check results

Commands, from the repository root (`check.sh` does all of this; it passes
`--fine-pitch-return-via-advisory` only when `FINE_PITCH_RETURN_VIA_ADVISORY=1`
is set, and the committed evidence was produced with it on):

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
| KiCad DRC, all severities | 0 errors, 47 warnings, 0 unconnected items | `reports/rtd_afe.kicad-drc.json` (47 `lib_footprint_mismatch`; `unconnected_items: []`; re-recorded 2026-11-10 from the edited board) |
| Schematic/PCB parity | empty | same DRC report |
| pcbnew reopen | OK | `check.sh` loads the board with `pcbnew.LoadBoard` |
| Ground | one `GND` plane; `AGND` and `DGND` are absent | `check.sh` asserts it; see below |
| KiCad ERC | 0 errors, 1 warning | `reports/rtd_afe.kicad-erc.json` |
| Synth routing | routed, 40 connected nets, 0 open | `reports/rtd_afe.routing.json` |
| Synth release (historical, source 01a77d54; PR status is untrusted, `release_ready: false`) | `production_status: production`, `release_ready: true` | `reports/release.json`; router and export gates only (see its `gate_scope`) |
| Synth signal integrity | 12 findings, 0 blocking: 11 `W-SYNTH-DRC-016` advisories, 1 parallel run | `reports/release.json` |

The delivered board is `kicad/rtd_afe.kicad_pcb`
(`5dd5b8a573fce34812835101d4bc451cd51973595fd41b4ef97da169f72d7810`).
The schematic is `kicad/rtd_afe.kicad_sch`
(`38a2e8b21dfb376404d721db81289cfd9f3bb1000bdc8980f3884a209ef4034e`).

### DRC history

The earlier 58 x 47, 60 x 49, 64 x 49 and 67 x 49 mm attempts failed on Synth's
ground-island repair: the split pours fragmented around U2. The compact rework
grouped the analog chain at the left, the MCU/flash/crystal and USB at the
right, and put J1/J2/J3 on the edges. On the merged toolchain the split-ground
version of this compact board still left two DGND pour islands near
(32.55, 32.75) and (41.45, 24.03) mm; no 0.6 mm tie via fitted. Replacing the
split with one GND plane removed the islands. The route then closed after three
placement moves that opened tight pockets: C_DV2 to (44.5, 26.5), L1 to
(43.5, 20.0) and C_VI to (45.0, 24.2), with the related R_RST/C_DV and L2
positions adjusted at the same time. That layout is historical. The delivered board has
0 errors and 0 unconnected items; its 47 warnings (all library comparison) are
dispositioned below.

A 12-test-point variant was then placed on the earlier notched outline: VREF,
GND, +3V3A, VBUS, +3V3, +3V3_RP_A, DVDD_1V1, RUN, USB_DP, USB_DN, SWDIO and
SWCLK. Its route failed on a TP6 edge clearance, a QSPI_SS/+3V3 clearance at U4
and two GND pour islands, so the test points are not in the delivered board.
The plain rectangle adds area, but the set has not been re-placed and rerouted;
see the README.

The branch placer exposed a sidecar trap: a floorplan whose entries are all
absolute is passed through `close_pinned_bands`, which slides the pinned parts
to shut gaps. It moved U4 onto U2 and produced 91 mask/clearance violations.
Keeping one relative entry (`C_FL` relative to `U4`), as the original sidecar
did, disables that pass; the floorplan then exported exactly as written. This is
recorded under Synth defects below.

The first clean route still had 56 silkscreen warnings because reference labels
sat on neighbouring pads. That revision moved U1's label to (24, 34.5) mm and
hid 25 passives/TVS and H1/H2/H3/J1; the delivered board had 0 silk warnings.

### The compact re-place

This rework fits the outline to the placement instead of spreading the parts
over a fixed 50.1 x 44.1 mm rectangle. The board is now 40.1 x 44.2 mm
(1772 mm²) and routes with 0 open nets, 0 DRC errors, 0 unconnected items
(47 warnings, see the table above). That does not prove it is the densest arrangement; it is one
successful FreeRouting run, and `check.sh` re-routes from scratch.

The largest bare rectangle inside the outline was 7 x 8 mm (56 mm²) on the earlier route (not re-measured after the C_IO, L1 and U4 moves), down from
19 x 19 mm (361 mm²) on the rectangle. The groups were re-packed, not scaled:
J2 is at (14.8, 41.7), the filter just above it, U1 at (12, 25.1); the reference
block is at the top-left (U3 at (10.5, 11.6), C_RI at (4.8, 11.3), L2 at (13, 6.5),
C_RO at (16, 13.6), R_RST at (19.8, 14), R_CC2 at (21, 11.5)); the digital
block is U2 at (28, 29.6), U4 at (22, 19), Y1 at (28, 36.3). The analog and
digital groups overlap in y, so the old 11 mm channel is gone.

J1 is centred on the top edge at (22.55, 6.0). Centring it on the 43.2 mm
version left the +3V3/DVDD escapes packed against U2's top pad row and the
router could not tie the resulting F.Cu GND fragment (no 0.6 mm via fits in a
0.4 mm-pitch pad row). The extra 1 mm of height and U2 at y = 29.6 give that
row room; one GND stitching via beside pad 47 ties the fragment. That is the
trade this revision makes for the centred connector.

The RTD and AIN lengths change with the re-pack: RTD_FP 16.78 mm, RTD_SP
6.75 mm, AIN1 12.39 mm, RTD_SN 6.94 mm, AIN2 13.55 mm, RTD_FN 15.46 mm. VREF is
37.85 mm of track on F.Cu with no via and C_RV stays at REFP0. J2 and J3 are
level at y = 41.7 mm, 0.71 mm from the bottom edge. This route has one
via-in-pad, on C_DV.2 at (16.138, 22.835) mm; `release.json` lists it as a
fabrication note (order with filled and capped vias).

## ERC findings and dispositions

`reports/rtd_afe.kicad-erc.json` has one warning and no errors:

| Warning | Disposition |
| --- | --- |
| `lib_symbol_mismatch`: Symbol `REF5025ID` does not match the copy in `Reference_Voltage` | The embedded symbol is generated from the registry pins, so its drawing differs from the installed library copy. Pin numbers and electrical types match the reviewed part; the netlist and PCB parity are clean. Cosmetic. |

It is a library-comparison warning from an embedded-symbol export, not a
connectivity finding. The registry now uses `Device:FerriteBead`, so the earlier
`Ferrite_Bead` warning is gone.

### KiCad DRC: 47 `lib_footprint_mismatch` warnings

`reports/rtd_afe.kicad-drc.json` has 0 errors, 0 unconnected items and 47
warnings, all `lib_footprint_mismatch`. The earlier `silk_edge_clearance` warning
(the C_DVB reference field clipped by the board edge) is gone after the label move.
The `lib_footprint_mismatch` warnings:

| Footprint | Count | Disposition |
| --- | --- | --- |
| `C_0603_1608Metric` (`Capacitor_SMD`) | 23 | Embedded footprint differs from the library copy; cause below. |
| `R_0603_1608Metric` (`Resistor_SMD`) | 11 | Same. |
| `C_0805_2012Metric` (`Capacitor_SMD`) | 2 | Same. |
| ICs, diode, ferrite, inductor and other footprints | 11 | Same. |

Cause: the exporter redraws each reference label at 0.8 mm text with an 0.18 mm
stroke (`LABEL_STROKE_MM`, commit a5821e5e) and removes library silkscreen lines
that would clip a pad or via. The previous committed board was exported before
that change and showed 0 warnings; a footprint diff of one part shows the label
size and stroke changed and two library silkscreen lines were dropped, with pads,
copper and drills unchanged in that part (one footprint was diffed; the others were not). Any board exported from the same Synth revision
shows the same warnings. These are not hidden: the `reports/` DRC file lists all
48. A human should confirm this disposition.

## Status against Synth main's return-via gate

The board does not pass Synth main's return-via gate on default settings. The
committed evidence was produced with the opt-in fine-pitch relaxation
(`--fine-pitch-return-via-advisory`, env `SYNTH_FINE_PITCH_RETURN_VIA_ADVISORY`),
which needs signal-integrity review sign-off; that sign-off is not given. With
the flag off the board ends `validation_failed` with 12 blocking return-via
findings, so it stays untrusted and not release-ready. The committed reports
were regenerated at source revision `01a77d54` (`check.sh` exit 0, routing state
`routed`, 0 open nets, 0 blocking reasons). Main's router treats a layer change
on a fast-edge net as blocking unless a ground-return via sits within 0.80 mm.
Without the flag, a re-run ends in `validation_failed` with 12 blocking
return-via findings. This branch carries two router commits (`island_vias.rs`):

- (a) With the flag on only, a return via with no legal spot inside a fine-pitch pad breakout (pads
  0.5 mm or closer, signal via within 2.5 mm of such a pad) is reported as an
  advisory instead of blocking.
- (b) When the usual 0.6 mm via has no room, a compact 0.46/0.20 mm via, held
  off other copper by the 0.13 mm clearance, is tried for return vias and island
  ties. Six vias in the delivered board are this size.

Placement tuning that was needed (sidecar): C_IO rotated to 270, L1 at x = 34.4
and U4 at x = 22.0.

Advisories (exist only with the flag on): 11 `W-SYNTH-DRC-016` (`reports/release.json`,
`reports/rtd_afe.connectivity.json`). The return loop is left open in 11 layer
changes, so they are for the SI review and are not closed:

| Net | Via position (mm) |
| --- | --- |
| SPI_MOSI | (23.41, 28.63) |
| SPI_SCK | (22.69, 27.91) |
| SPI_MISO | (23.41, 27.08) |
| ADC_DRDY | (15.49, 24.64), (23.08, 30.50) |
| ADC_START | (14.32, 29.00), (23.47, 29.71) |
| ADC_RESET_N | (25.54, 29.05) |
| CC1 | (23.76, 7.95) |
| USB_DN | (27.96, 27.12) |
| DVDD_1V1 | (25.86, 30.42) |

## Synth signal-integrity findings

`reports/release.json` lists 12 non-blocking signal-integrity findings: the 11
`W-SYNTH-DRC-016` advisories above and 1 parallel-run finding, SPI_MISO and
ADC_DRDY running 3.5 mm on B.Cu 0.203 mm apart (within a dielectric height). No
RTD_FP/RTD_SP/RTD_SN/RTD_FN, AIN1, AIN2 or VREF finding appears. The six
RTD/input nets and VREF are on F.Cu only. QSPI_SS/SCLK/SD0/SD3 and the crystal
nets also stay on F.Cu.

The USB path is the one an SI reviewer should look at first: it is not
length-matched. The MCU-side and connector-side lengths add to 26.65 mm on DP
(13.59 + 13.06) and 27.96 mm on DN (15.40 + 12.56), a 1.31 mm difference. USB_DN
changes layer (advisory above). A return via next to each advisory layer change
could not be fitted by the router; fixing that is a layout change that needs an
SI review first.

## Synth validate warnings

`target/debug/synth validate` (with an empty `XDG_DATA_HOME`) also emits
warnings. They are not part of the DRC acceptance bar, but they are not hidden:

| Finding | Disposition |
| --- | --- |
| `E-SYNTH-POWER-004` on VBUS: 7 load endpoints, 1 capacitor | Heuristic. C_L1 (2.2 uF) is the LDO input capacitor. There is no separate VBUS bulk capacitor, fuse or TVS; that is a real gap, see the USB section. |
| `W-SYNTH-ANOMALY-001` | Statistical topology heuristic (score -2.43 against a 0 boundary); no action. |
| `W-SYNTH-SUPPLY-002` x10 | Generic parts (passives, headers, ESD) have no distributor identity. |
| `W-SYNTH-PART-UNVERIFIED` | Not re-run on the final board; Y1 (`ecs_120_20_33_tr`) now has a part-level review in the registry (2026-11-05). |
| `E-SYNTH-CONNECT-007` x2 | `SPI_MISO` and `ADC_DRDY` join a `bidirectional` MCU pin to an ADC `output`. Normal SPI/ready wiring; the MCU pin is only an input in those modes. |
| `E-SYNTH-ESD-001` x3 | VBUS has no reverse-polarity protection and CC1/CC2 have no TVS. Real gaps; see USB. |
| `E-SYNTH-SCHEM-003` x11 | Decoupling-cap symbol bodies are more than 25 mm from their IC bodies in the compiler-drawn schematic. A schematic-drawing warning, not the PCB. |
| `E-SYNTH-SCHEM-004`, `E-SYNTH-SCHEM-005` | One 119.4 mm drawn wire and one 4-way junction in the compiler-drawn schematic. Drawing only. |

## JLCDFM findings accepted

JLCDFM on the Gerbers of this board (2026-11-04) reports:

- **Slot width check, 4 danger items.** These are the four shield slots of J1 (TYPE-C-31-M-12, LCSC C165948), drilled 0.60 x 1.70 mm (two) and 0.60 x 1.20 mm (two) with 0.90 mm pads. The Korean Hroparts datasheet "Recommended P.C.B. layout (component side)" shows the same 0.60 mm slot, 0.90 mm pad and 1.70 mm drill length, with a layout tolerance of +/- 0.05 mm. The KiCad library footprint matches the datasheet, so the slots are left at 0.60 mm. JLCDFM's danger threshold for slot width is below 0.61 mm; the flag is a generic fabrication limit and is accepted here. If JLCPCB engineering queries the slots, the answer is this datasheet dimension.
- **Trace width, 72 warnings (0.13 mm).** These are the router's default 0.127 mm (5 mil) signal width, which JLCDFM rates as a warning below 0.13 mm. JLCPCB's stated minimum trace width for this stackup is lower, so the board is manufacturable; the warnings are not changed.

## Per-part datasheet review

"Reviewed" below means the registry's `provenance.reviewed_by` is set. Steve
Munene reviewed U1, U3 and U5 on 2026-10-27 and ecs_120_20_33_tr, esd_usb,
ferrite_bead_0603, header_1x3, header_1x4, l_generic_0805, rp2350,
usb_c_receptacle, w25q128_flash_208mil and mounting_hole_m2_5_clearance on
2026-11-05. r_generic_0603, c_generic_0603 and c_generic_0805 were reviewed
upstream by JeffMboya on 2026-11-01. The datasheet pin tests
(`rtd_front_end_parts_match_their_datasheet_pinouts` for U1/U3,
`ble_node_parts_match_their_datasheet_pinouts` for U5) check the listed key
pins. It does not mean this work re-read the whole datasheet or that a second
person checked it. The HRO (J1) and Tech Public (D1/D2) datasheets were read as
scans.

Review findings of 2026-11-05: the RP2350 LCSC number was a capacitor number and
is now C42411118, and its SPI0 pin tags are corrected (GPIO0 MISO, GPIO3 MOSI).
U4 uses the new part `w25q128_flash_208mil` (SOIC-8 208 mil,
`Package_SO:SOIC-8_5.3x5.3mm_P1.27mm`, 2.7 to 3.6 V) because the 150 mil
footprint was wrong for the W25Q128JVSIQ. `esd_usb` max_voltage_v is 5.0
(ESD5V0B03-323: VRWM 5 V, bidirectional, 0.3 pF, IPP 4 A, clamp 10 V at 1 A) and
the old 100 V / 1000 mA fields are removed. `usb_c_receptacle` is 16-pin (HRO
TYPE-C-31-M-12); the drawing's pad order matches the KiCad footprint, but the
power-pad spans are 4.90/6.50 mm against 4.80/6.40 mm on the drawing, unexplained
and within the pad width. The mounting holes H1-H4 use the 2.7 mm M2.5 clearance
hole (previously 2.5 mm, no clearance).

| Ref | Part / MPN | Datasheet | Pins checked | Decoupling / support | Review |
| --- | --- | --- | --- | --- | --- |
| U1 | TI ADS124S08IRHBR (RHB VQFN-32) | TI SBAS660C, https://www.ti.com/lit/ds/symlink/ads124s08.pdf | AINCOM 1, AIN0 7, CS 9, DOUT/DRDY 12, DGND 14, DVDD 16, CLK 17, REFOUT 23, NC 25, AVDD 26, AVSS 27, REFP1/AIN6 32, EP 33; 13 of 33 checked | C_AV1 330 nF + C_AV2 10 uF on AVDD (+3V3A); C_DV 100 nF DVDD; C_IO 100 nF IOVDD; C_RO 1 uF REFOUT | Reviewed, registry `ads124s08` |
| U3 | TI REF5025AIDR (SOIC-8) | TI SBOS410I, https://www.ti.com/lit/ds/gpn/ref5025 | VIN 2, TEMP 3, GND 4, VOUT 6, NC 7; 5 of 8 | C_RI 1 uF VIN, C_RV 1 uF VOUT | Reviewed, registry `ref5025` |
| U2 | Raspberry Pi RP2350A (QFN-60) | https://datasheets.raspberrypi.com/rp2350/rp2350-datasheet.pdf (from the KiCad symbol; not opened in this work) | Pin numbers compared against the installed KiCad `MCU_RaspberryPi:RP2350A` symbol: GPIO0 2, GPIO1 3, GPIO2 4, GPIO3 5, USB_DM 51, USB_DP 52, VREG_AVDD 46, VREG_PGND 47, VREG_LX 48, VREG_VIN 49, VREG_FB 50, IOVDD 1/11/20/30/38/45, DVDD 6/23/39. The registry's own note claiming RP2040 numbering is stale | C_IO1..C_IO4 100 nF on IOVDD (4 for 6 pins); C_DV1/C_DV2 100 nF + C_DVB 4.7 uF on DVDD 1.1 V (3 pins); C_VI 4.7 uF VREG_VIN; R_AV 33R + C_AVF 4.7 uF on VREG_AVDD/ADC_AVDD; L1 3.3 uH on VREG_LX | Reviewed, registry `rp2350` (2026-11-05) |
| U4 | Winbond W25Q128JVSIQ (SOIC-8 208 mil, LCSC C97521) | Registry `w25q128_flash_208mil` | CS 1, IO1 2, IO2 3, GND 4, IO0 5, CLK 6, IO3 7, VCC 8; all 8 used | C_FL 100 nF | Reviewed, registry `w25q128_flash_208mil` (2026-11-05) |
| U5 | TI LP5907MFX-3.3 (SOT-23-5) | TI SNVS798Q, https://www.ti.com/lit/ds/symlink/lp5907.pdf | VIN 1, GND 2, EN 3, VOUT 5; pin 4 modelled NC/open | C_L1 2.2 uF VIN, C_L2 2.2 uF VOUT | Reviewed, registry `lp5907_3v3` |
| J1 | HRO TYPE-C-31-M-12 (USB-C) | None in the registry; LCSC C165948 | 16 signal/power pins plus 4 shield pads: A1/A12/B1/B12/SH GND, A4/A9/B4/B9 VBUS, A5 CC1, B5 CC2, A6/B6 DP, A7/B7 DN; SBU1/SBU2 open | 5.1k CC1/CC2; 27R DP/DN; D1/D2 TVS | Reviewed, registry `usb_c_receptacle` (2026-11-05) |
| D1, D2 | `esd_usb`, ESD5V0B03-323 (SOD-323, LCSC C521915) | Tech Public datasheet (read as a scan) | IO to DP/DN, GND | — | Reviewed, registry `esd_usb` (2026-11-05) |
| Y1 | ECS-120-20-33-TR, 12 MHz, CL 20 pF | https://ecsxtal.com/store/pdf/ecx-32.pdf | Pads 1/3 crystal, 2/4 ground, per the registry description; not re-read in this work | C_X1/C_X2 33 pF C0G; R_X 1k on XOUT | Reviewed, registry `ecs_120_20_33_tr` (2026-11-05); LCSC C2449814 |
| L2 | BLM18AG601SN1D, 0603 ferrite bead, 600 ohm at 100 MHz (C19330) | None | 2 pins | — | Reviewed, registry `ferrite_bead_0603` (2026-11-05) |
| L1 | Sunlord MPL2012S3R3MHT 3.3 uH 0805 (LCSC C98331) | None | 2 pins | — | Reviewed, registry `l_generic_0805` (2026-11-05) |
| R_S1/R_S2, R_BIAS, R_AV, R_RST, R_RUN, R_X, R_CC1/2, R_DP/DN | Generic 0603 | None | 2 pins | Values in the BOM; R_S1/R_S2 and R_BIAS are 0.1 % in the source | Reviewed upstream, `r_generic_0603` (JeffMboya, 2026-11-01) |
| C_* | Generic 0603/0805 | None | 2 pins | C0G for the filter and crystal; X7R for the supply decoupling | Reviewed upstream, `c_generic_0603`/`c_generic_0805` (JeffMboya, 2026-11-01) |
| J2, J3 | 2.54 mm headers (1x4, 1x3) | None | 4 and 3 pins | — | Reviewed, registry `header_1x4`/`header_1x3` (2026-11-05) |
| H1..H4 | M2.5 clearance NPTH (2.7 mm) | None | 1 hole | — | Reviewed, registry `mounting_hole_m2_5_clearance` (2026-11-05) |

The registry's required decoupling is met for ADS124S08 (AVDD 330 nF, DVDD
100 nF), REF5025 (VIN/VOUT 1 uF), LP5907 (VIN/VOUT 1 uF, 2.2 uF fitted) and
W25Q128 (VCC 100 nF). It is **unmet for the RP2350**: `parts/mcus/rp2350.synth.toml`
requires 7 IOVDD and 3 DVDD 100 nF caps; the board has 4 IOVDD and 2 DVDD (plus C_DVB bulk). This is an open SI/PI gap. It is also below
Raspberry Pi's per-pin recommendation: the board has 4 IOVDD caps (C_IO1..C_IO4)
for 6 IOVDD pins, and 2 DVDD caps (C_DV1, C_DV2) plus C_DVB bulk for 3 DVDD pins,
while the hardware design guide (section 2.2.1) recommends one 100 nF per power
pin with pins 53/54 sharing one. Four more caps could not be placed without
breaking routing; this is left for the SI/PI review. Proximity of each
support part to the pin it serves was measured on the earlier split-ground
route; the single-ground rework moved C_DV2, C_VI and L1, so those distances
are not carried forward as verified numbers.

The BOM (`kicad/bom.csv`) now carries `dielectric`, `voltage`, `tolerance` and
`power` columns copied from the structured attributes in `rtd_afe.synth`, so the
C0G filter capacitors and the 0.1 % resistors are visible to an assembler that
reads only the BOM. Empty cells mean the source did not declare that attribute.

## Sourcing pins

The listed passives, J3, U5 (C80670, TI LP5907MFX-3.3/NOPB), L2 (C19330, BLM18AG601SN1D), D1/D2 (C521915, ESD5V0B03-323), U1 (C2871942), U3 (C11341), Y1 (C2449814, ECS-ECS-120-20-33-TR), L1 (C98331, Sunlord MPL2012S3R3MHT) and C_D (C14663, Yageo CC0603KRX7R9BB104) are pinned in `rtd_afe.synth` by explicit `lcsc_pn` and `mpn`, and `kicad/bom.csv` carries them (parts with different LCSC numbers stay on separate BOM lines). Y1, L1 and C_D were pinned after the last export, so `kicad/bom.csv`, the gerbers and the reports do not carry those three numbers yet. A re-export on the current tree does not route this board (FreeRouting leaves +3V3 at U2-49/U2-53 and VREG_LX open with L1 at x = 34.4; moving L1 to (32.0, 22.0) routes it but leaves 1 to 3 return-via items), so the evidence stays at the earlier export until the board is re-laid out. Unpinned and matched by JLCPCB by value, which is unsafe (JLCPCB matched the wrong dielectric and values in a DFM test):

- C_D is X7R, not C0G: no 100 nF C0G 50 V part is stocked in 0603 at LCSC (only 1206, e.g. C170182). A 1206 C_D was tried and the router then failed to connect +3V3 and VREG_LX, so it was reverted. This is a deviation from the C0G filter intent; the X7R DC-bias drift at the low differential level was not measured.
- L1 C98331 (Sunlord MPL2012S3R3MHT, 3.3 uH, 190 mOhm, 900 mA, 0805) is picked for the footprint; its DCR and saturation current were not checked against the RP2350 datasheet. The Pi-recommended part is Abracon AOTA-B201610S3R3-101-T (C42411119); its 2016 package does not fit the 0805 footprint.
- Y1 C2449814 is an exact MPN match (49 in stock when checked 2026-11-06).
- U2: the registry LCSC number is now C42411118 (see the review findings above); it was not checked against a stock listing.

The registry LCSC number for `lp5907_3v3` (C83437) was wrong; the pinned C80670 is the TI LP5907MFX-3.3/NOPB. The part-level registry reviews are in the table above; nobody has signed off these pins against a fab-house BOM check.

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

Issue #363 asked for an AGND/DGND split. The issue owner has not signed off on
this deviation.

Why one plane: the ADC's digital return (DGND pin 14), CLK, AVSS, AVSS_SW,
REFN0, REFCOM and exposed pad return to the same plane as the MCU, flash, USB
and LDO, so a signal that crosses the analog/digital boundary never has to
change reference planes. The earlier split version needed a tie and still left
two isolated pours on the merged router. No impedance or ground-noise
measurement was made; this is the main item for the analog/SI review.

## Measured layout numbers

- 389 track segments, 89 vias: 60 GND, 29 signal. 688.4 mm of track. Six vias
  are the compact 0.46/0.20 mm size, 83 are 0.6/0.3 mm. 40 nets, 0 open. The
  router imported 71 of the vias; the others are not router-imported (source
  stitching and repair vias).
- Analog nets, all F.Cu only, no via:

| Net | Length |
| --- | --- |
| RTD_FP | 16.78 mm |
| RTD_SP | 6.75 mm |
| AIN1 | 12.39 mm |
| RTD_SN | 6.94 mm |
| AIN2 | 13.55 mm |
| RTD_FN | 15.46 mm |
| VREF | 37.85 mm |

- SPI_SCK/MOSI/MISO/CS are 13.68/12.01/10.96/14.22 mm, all four change layer.
  QSPI_SS/SCLK/SD0/SD1/SD2/SD3 are 11.95/7.07/5.58/11.81/11.98/8.50 mm; SD1 and
  SD2 change layer, the others stay on F.Cu.
- USB: the RP2350-side nets are 13.59 mm (DP) and 15.40 mm (DN); the
  connector-side nets are 13.06 mm and 12.56 mm. Total path 26.65 mm (DP) and
  27.96 mm (DN), a 1.31 mm mismatch, full speed. Only USB_DN changes layer; the
  pair is not routed as a coupled pair and has no controlled impedance.
- Crystal: XIN 11.49 mm, XOUT 5.09 mm, XOUT_R 19.29 mm, all F.Cu.
- 34 reference labels are hidden: C_AV1, C_AVF, C_CM1, C_FL, C_CM2, C_D, C_DV,
  C_DV1, C_DV2, C_IO, C_IO1, C_IO2, C_IO3, C_IO4, C_L2, C_RO, C_RV, C_VI, D1,
  D2, H1, H2, H3, J1, J3, R_AV, R_BIAS, R_CC2, R_DN, R_DP, R_RST, R_S1, R_S2,
  R_X. They remain in the BOM, PnP and schematic.
- Board outline and stackup above; 53 footprints.

## Left to a human

Nothing below is signed off. The list is deliberately blunt.

- **USB-C 3D model (review comment, partly fixed)**: `kicad/rtd_afe.step` and the 3D
  `images/` now use the GrabCAD HRO TYPE-C-31-M-12 model in `../models/` (offset and
  source in `../models/README.md`), rendered from a scratch copy of the board. Still
  open: the committed `kicad_pcb` names the old placeholder path at
  `/usr/share/kicad/...` (the exporter has no custom-model path), and the website PR
  has no USB-C body at all. The placeholder block in `agent-run/step-models/` is
  historical.
- **C_DVB reference label was cut by the board edge (review comment, fixed by hand)**:
  the label sat at +4.05 mm from C_DVB (x = 37.0), spanning about x = 39 to 43 against an
  edge at 42.5 (the review image reads it as "C_DVE"). It is now at (+1.5, -2.2) mm, set by
  `[labels.C_DVB]` in `rtd_afe.placement.layout.toml` and written into the committed
  `kicad/rtd_afe.kicad_pcb` by editing that one reference-label `(at …)`; no other line of the
  board changed. This is a hand edit: the tree does not route, so the package could not be
  regenerated (a re-export on 2026-11-10 left +3V3 and VREG_LX open). A fresh `kicad-cli pcb drc`
  on the edited board shows only the 47 `lib_footprint_mismatch` entries; the committed
  `reports/rtd_afe.kicad-drc.json` and `kicad/gerbers/rtd_afe-F_Silkscreen.gbr` were
  re-recorded from the edited board on 2026-11-10 (`kicad-cli pcb drc`, and `pcb export gerbers`
  with the exporter's flags `--no-protel-ext --subtract-soldermask`); a re-export of the parent
  board with the same flags reproduced every other committed Gerber and drill file, so only
  F.Silkscreen changed (the others keep their original timestamps). The three `images/pcb-3d-*.png` were re-rendered from the edited board.
- **SI/PI/RF review (designated reviewer)**: the 24-bit analog layout, the
  single ground plane, the 37.85 mm of VREF track from U3 to C_RV, the RC filter
  against the chosen ADC data rate, the IDAC output's 10 nF load and settling,
  the PGA common-mode range at the chosen gain, the unused-input termination, the
  11 return-via advisories, the SPI_MISO/ADC_DRDY parallel run, the RP2350
  decoupling count and the 1.31 mm USB length mismatch. None of these were simulated or measured.
- **Metrology/analog**: the measurement is not ratiometric. The IDAC's absolute
  accuracy and drift, the REF5025 grade and drift, the 1 k filter resistors and
  the PT100 wiring resistance set the accuracy; there is no calibration or
  error budget. A 4-wire measurement with 0.1 % resistors is not automatically a
  0.1 % thermometer.
- **USB/ESD**: no VBUS fuse, TVS or reverse-polarity protection; CC1/CC2 have no
  TVS; the shield is tied directly to GND; the D1/D2 BOM value text is a
  placeholder (the MPN is pinned). USB-C and ESD
  compliance were not checked.
- **Mechanical**: the measured margins are in the table above. In short, the
  mounting holes leave 2.15 to 2.24 mm of material around a 2.7 mm hole, the
  bottom connectors are 0.71 mm from the edge, and J1 is 1.27 mm from the top
  edge and centred. J1 centring, the enclosure,
  screw-head clearance, connector overhang and header orientation were not
  checked. Synth places no board-name or revision silk.
- **Test points**: none are fitted. The 12-pad set was tried on the earlier
  outline and left GND islands; the top-left area it used now holds the
  reference cluster, so a new set needs a different location and a fresh route.
  A bench bring-up has to probe component pads or a harness; the set is a future
  layout task.
- **Fabrication/DFM**: no fab house has seen the Gerbers, drill, BOM or PnP.
  The 0.4 mm QFN-60, the 0.5 mm VQFN-32 and the 0.5 mm USB-C need a fab
  capability check; the stackup is a design note, not in the exported board, and there is no
  controlled-impedance quote. The source declares `requires via_in_pad`
  (manufacturer `jlc-filled-via`); this route has no via inside a pad, but a
  re-route can bring one back and JLCPCB has not accepted that process. Assembly
  of the generic passives needs the C0G/tolerance attributes from the source,
  not just the BOM values.
- **Firmware**: the RP2350 boot, QSPI flash, ADC gain/data-rate/IDAC registers,
  USB descriptors and the PT100 conversion/calibration do not exist in this
  repository. Nothing was powered.
- **Sourcing**: C_D (C14663, X7R), L1 (C98331) and Y1 (C2449814) are pinned in
  the source (see Sourcing pins), but the committed `kicad/bom.csv` predates the
  pins (C_D C0G 50 V no LCSC; L1 and Y1 no LCSC) and must not be used for
  assembly until regenerated. The RP2350 decoupling is below the registry
  requirement (7 IOVDD, 3 DVDD) and the Pi recommendation.
- **Independent sign-off**: the board review is with the designated reviewer.
  Part-level registry reviews exist for the parts in the table above; they are
  single-reviewer reviews. The issue owner has not signed off on the single
  GND plane against #363. There has been no second-person check of this record
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
   return-via half of this repair is advisory only with the opt-in flag (see item 7).
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
   (fixed for this board).** The earlier notch was an `[[outline]]` path in
   `rtd_afe.placement.layout.toml`. The final board is a plain fitted rectangle,
   so the sidecar carries no outline and a normal `synth export-kicad` needs no
   post-export cut. The feature remains for boards that need a non-rectangular
   outline; a `.synth` outline statement is still not implemented.
7. **Return-via misses blocked a whole candidate (fixed).** The island repair
   reported a signal via with no ground return as an unresolved island, so a
   route with clean connectivity could not be promoted. A missing return via is
   the existing `W-SYNTH-DRC-016` SI warning, so it is advisory (for fine-pitch breakouts only with the opt-in
   `--fine-pitch-return-via-advisory`, which needs SI sign-off); an untied
   copper island still blocks. This branch adds two router commits on top of that
   (see the return-via gate section). The regression test is
   `an_unreturned_signal_via_is_advisory_not_blocking`.
8. **Source ground fence ignored the sidecar outline (fixed, historical).** On
   the earlier notched layout the fence vias were placed against the fitted
   rectangle, so three landed on or outside the notch edge (0.0/0.26 mm
   copper-to-edge). The fence now follows an `[[outline]]` path and skips points
   outside it; the final rectangular board has no custom outline and no
   edge-clearance error.

No GitHub issue numbers are linked for the Synth defects above.

## Reproducibility

Hashes of the committed inputs and outputs are in `SHA256SUMS` (the single
source of truth; this file no longer copies them, so they cannot drift). Verify with:

```sh
grep -v '^#' SHA256SUMS | sha256sum -c -
```

`check.sh` sets `SYNTH_SOURCE_REVISION` from the checked-out commit, and both
`release.json` and the routing provenance record it. It also rewrites the
checkout path in `release.json`, `rtd_afe.routing.json` and `rtd_afe.net` to a
repository-relative one, matching the usb-uart board's convention. The
`source_revision` (01a77d54) names the source commit that produced the evidence.
The head source differs from it (C_D X7R, Y1/L1/C_D pins in `rtd_afe.synth`), so
the evidence is not from the head source. All other reports are as the tools wrote them.
`SHA256SUMS` covers all deliverables.

The failed attempts described above did **not** route clean; the committed
placement did. I did not run the final inputs three times to claim a
byte-identical board, so the committed board is one successful run, not a
proven reproducible one. **Reproducibility is OPEN**: a re-export on the current
tree does not route (+3V3 and VREG_LX open with L1 at x = 34.4), so the committed
evidence is historical. FreeRouting 2.4.1 and KiCad 10.0.7 on another machine
may produce a different route. `check.sh` is the repeatable acceptance check;
it re-routes from the sources and fails on any DRC error, unconnected item, or
ground violation.
