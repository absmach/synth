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

- Outline: 40.1 x 44.2 mm rectangle, 1772 mm². A plain rectangular
  `[[outline]]` runs x 2.50 to 42.59 mm, y -0.01 to 44.21 mm; there is no notch
  and no post-export cut.
- Four copper layers with an explicit 1.5862 mm FR4 stackup: 0.035 mm copper,
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
| H1 | centre (6, 3.5); 3.50 mm from left edge, 3.51 from top; 2.25/2.26 mm of material | not checked |
| H2 | centre (39, 3.5); 3.59 mm from right edge, 3.51 from top; 2.34/2.26 mm of material | not checked |
| H3 | centre (6, 40.7); 3.50 mm from left edge, 3.51 from bottom; 2.25/2.26 mm of material | not checked |
| H4 | centre (39, 40.7); 3.59 mm from right edge, 3.51 from bottom; 2.34/2.26 mm of material | not checked |
| J1 | body bbox x 17.21..27.89, y 1.26..10.73; 1.27 mm from top edge; centred on the board (centre x = 22.545) | overhang/enclosure not checked |
| J2 | body bbox x 9.20..20.41, y 39.90..43.50; 0.71 mm from bottom edge | mating fit not checked |
| J3 | body bbox x 23.66..32.34, y 39.90..43.50; 0.71 mm from bottom edge | mating fit not checked |

The mounting holes are non-plated, 2.5 mm, with 5.6 x 5.6 mm all-layer rule
areas. J1 is centred on the top edge. The enclosure and screw-head clearance
are open mechanical items.

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
| KiCad DRC, all severities | 0 errors, 47 warnings, 0 unconnected items | `reports/rtd_afe.kicad-drc.json` (all 47 are `lib_footprint_mismatch`; `unconnected_items: []`) |
| Schematic/PCB parity | empty | same DRC report |
| pcbnew reopen | OK | `check.sh` loads the board with `pcbnew.LoadBoard` |
| Ground | one `GND` plane; `AGND` and `DGND` are absent | `check.sh` asserts it; see below |
| KiCad ERC | 0 errors, 1 warning | `reports/rtd_afe.kicad-erc.json` |
| Synth routing | routed, 40 connected nets, 0 open | `reports/rtd_afe.routing.json` |
| Synth release | `production_status: untrusted`, `release_ready: false` | `reports/release.json`; the unverified-crystal override, not a DRC failure |
| Synth signal integrity | 18 warnings, 0 blocking | `reports/release.json` |

The delivered board is `kicad/rtd_afe.kicad_pcb`
(`ed06039e52e1ec9a72fafea0f3b1772fe2497282d554f5f01d9611a7d176768c`).
The schematic is `kicad/rtd_afe.kicad_sch`
(`e61fde7f274ebf1bb58eefbb3c94e629d8b6a7402477d52c63bb6558daccedf2`).

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
positions adjusted at the same time. The delivered board has 0 errors and 0
unconnected items; its 47 library-comparison warnings are dispositioned below.

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
(47 library-mismatch warnings, see the table above). That does not prove it is the densest arrangement; it is one
successful FreeRouting run, and `check.sh` re-routes from scratch.

The largest bare rectangle inside the outline is 7 x 8 mm (56 mm²), down from
19 x 19 mm (361 mm²) on the rectangle. The groups were re-packed, not scaled:
J2 is at (14.8, 41.7), the filter just above it, U1 at (12, 25.1); the reference
block is at the top-left (U3 at (10.5, 11.6), C_RI at (4.8, 11.3), L2 at (13, 6.5),
C_RO at (16, 13.6), R_RST at (19.8, 14), R_CC2 at (21, 11.5)); the digital
block is U2 at (28, 29.6), U4 at (21.5, 19), Y1 at (28, 36.3). The analog and
digital groups overlap in y, so the old 11 mm channel is gone.

J1 is centred on the top edge at (22.55, 6.0). Centring it on the 43.2 mm
version left the +3V3/DVDD escapes packed against U2's top pad row and the
router could not tie the resulting F.Cu GND fragment (no 0.6 mm via fits in a
0.4 mm-pitch pad row). The extra 1 mm of height and U2 at y = 29.6 give that
row room; one GND stitching via beside pad 47 ties the fragment. That is the
trade this revision makes for the centred connector.

The RTD and AIN lengths change with the re-pack: RTD_FP 16.78 mm, RTD_SP
6.75 mm, AIN1 12.39 mm, RTD_SN 6.94 mm, AIN2 13.55 mm, RTD_FN 15.46 mm. VREF is
17.88 mm on F.Cu with no via and C_RV stays at REFP0. J2 and J3 are level at
y = 41.7 mm, 0.71 mm from the bottom edge. There is no via-in-pad in this route;
one source stitching via sits beside U2 pad 47.

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
warnings, all `lib_footprint_mismatch`:

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
47. A human should confirm this disposition.

## Status against Synth main's return-via gate

The committed evidence was produced at source revision `ee3027c6`, before
main's current router was merged into this branch. Main's router now treats
layer changes on fast-edge nets as blocking unless a ground-return via sits
within 0.80 mm. Re-running `check.sh` on the merged branch (merge commit
`efa65151`) ends in routing state `validation_failed`, with 12 blocking
findings of the form "signal via at (x, y) mm has no ground return via within
0.80 mm, and no spot in the GND pour holds one clear of the pads, vias and
tracks beside it". The vias are at (15.47, 26.34), (16.60, 24.77),
(14.53, 28.93), (15.50, 23.66), (25.46, 30.00), (25.04, 24.91),
(25.32, 23.89), (23.95, 8.05), (27.55, 27.30), (25.51, 28.31),
(29.11, 27.31) and (30.04, 27.20). KiCad DRC on that run still showed 0 errors,
0 unconnected items and the same 47 library-comparison warnings. The result is
deterministic.

So the committed board and reports have not been regenerated under main's
current router, and a fresh export of this source will not pass that gate until
the return vias are added or the nets are classified. This is the same gap as
the 15 layer-change warnings below. It is open work, not a pass.

## Synth signal-integrity warnings

`reports/release.json` lists 18 non-blocking signal-integrity findings: 15
layer changes with no ground-return via within 0.64 mm and 3 parallel-run
warnings. The layer changes are on SPI_MOSI/CS/MISO, ADC_DRDY, ADC_START,
QSPI_SD0/SD1/SD3, +3V3_RP_A, DVDD_1V1 and USB_DN_C. The parallel-run findings
are REFOUT/+3V3_RP_A (2.7 mm), ADC_START/VBUS (2.5 mm) and +3V3/VREG_LX
(4.6 mm at 0.147 mm). No RTD_FP/RTD_SP/RTD_SN/RTD_FN, AIN1, AIN2 or VREF finding
appears. The six RTD/input nets and VREF are on F.Cu only; SPI_SCK and QSPI_SD2
also stay on F.Cu.

The +3V3/VREG_LX run and the USB path are the ones an SI reviewer should look
at first: a regulator switching node beside the 3.3 V rail, and a USB path that
is not length-matched. The MCU-side and connector-side lengths add to 22.60 mm
on DP and 29.69 mm on DN, a 7.09 mm difference. A return via next to each layer
change would help; this was not added, because it is a layout change that needs
an SI review first.

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

## Sourcing pins

The listed passives, J3, U5 (C80670, TI LP5907MFX-3.3/NOPB), L2 (C19330, BLM18AG601SN1D) and D1/D2 (C521915, ESD5V0B03-323) are now pinned in `rtd_afe.synth` by explicit `lcsc_pn` and `mpn`, and `kicad/bom.csv` carries them (parts with different LCSC numbers stay on separate BOM lines). C_D, L1, Y1 and U2 are still unpinned and are matched by JLCPCB by value. That is unsafe: JLCPCB matched the wrong dielectric and values in a DFM test.

Not reviewed: the ESD5V0B03-323 datasheet used for D1/D2 has NOT been reviewed. Its capacitance (0.3 pF) and its pinout for USB D+/D- are unconfirmed. The registry LCSC number for `lp5907_3v3` (C83437) was wrong; the pinned C80670 is the TI LP5907MFX-3.3/NOPB. No reviewer has signed off these pins.

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

- 371 track segments, 77 vias: 52 GND, 25 signal. 654.8 mm of track. 40 nets, 0
  open. The router records 67 of the vias; the other ten are source ground
  stitching.
- Analog nets, all F.Cu only, no via:

| Net | Length |
| --- | --- |
| RTD_FP | 16.78 mm |
| RTD_SP | 6.75 mm |
| AIN1 | 12.39 mm |
| RTD_SN | 6.94 mm |
| AIN2 | 13.55 mm |
| RTD_FN | 15.46 mm |
| VREF | 17.88 mm |

- SPI_SCK is 11.00 mm; SPI_MOSI/MISO/CS are 10.96/10.88/11.07 mm. SCK stays on
  F.Cu; the other three change layer. QSPI_SS/SCLK/SD0..SD3 are
  11.74/7.80/6.25/11.37/11.17/9.24 mm; SD0, SD1 and SD3 change layer.
- USB: the RP2350-side nets are 13.08 mm (DP) and 17.11 mm (DN); the
  connector-side nets are 9.52 mm and 12.58 mm. Total path 22.60 mm (DP) and
  29.69 mm (DN), a 7.09 mm mismatch, full speed. Both sides change layers; the
  pair is not routed as a coupled pair and has no controlled impedance.
- Crystal: XIN 11.53 mm, XOUT 5.08 mm, XOUT_R 19.28 mm. XOUT_R changes layer;
  that is in the SI warnings above.
- 34 reference labels are hidden: C_AV1, C_AVF, C_CM1, C_FL, C_CM2, C_D, C_DV,
  C_DV1, C_DV2, C_IO, C_IO1, C_IO2, C_IO3, C_IO4, C_L2, C_RO, C_RV, C_VI, D1,
  D2, H1, H2, H3, J1, J3, R_AV, R_BIAS, R_CC2, R_DN, R_DP, R_RST, R_S1, R_S2,
  R_X. They remain in the BOM, PnP and schematic.
- Board outline and stackup above; 53 footprints.

## Left to a human

Nothing below is signed off. The list is deliberately blunt.

- **SI/PI/RF review (designated reviewer)**: the 24-bit analog layout, the
  single ground plane, the 17.88 mm VREF run from U3 to C_RV, the RC filter
  against the chosen ADC data rate, the IDAC output's 10 nF load and settling,
  the PGA common-mode range at the chosen gain, the unused-input termination, the
  15 layer-change warnings, the three parallel-run findings and the 7.09 mm USB
  length mismatch. None of these were simulated or measured.
- **Metrology/analog**: the measurement is not ratiometric. The IDAC's absolute
  accuracy and drift, the REF5025 grade and drift, the 1 k filter resistors and
  the PT100 wiring resistance set the accuracy; there is no calibration or
  error budget. A 4-wire measurement with 0.1 % resistors is not automatically a
  0.1 % thermometer.
- **USB/ESD**: no VBUS fuse, TVS or reverse-polarity protection; CC1/CC2 have no
  TVS; the shield is tied directly to GND; D1/D2 have no MPN. USB-C and ESD
  compliance were not checked.
- **Mechanical**: the measured margins are in the table above. In short, the
  mounting holes leave 2.25 to 2.34 mm of material around a 2.5 mm hole, the
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
  capability check; the four-layer stackup is declared by the source, not a
  controlled-impedance quote. The source declares `requires via_in_pad`
  (manufacturer `jlc-filled-via`); this route has no via inside a pad, but a
  re-route can bring one back and JLCPCB has not accepted that process. Assembly
  of the generic passives needs the C0G/tolerance attributes from the source,
  not just the BOM values.
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
   (fixed for this board).** The earlier notch was an `[[outline]]` path in
   `rtd_afe.placement.layout.toml`. The final board is a plain fitted rectangle,
   so the sidecar carries no outline and a normal `synth export-kicad` needs no
   post-export cut. The feature remains for boards that need a non-rectangular
   outline; a `.synth` outline statement is still not implemented.
7. **Return-via misses blocked a whole candidate (fixed).** The island repair
   reported a signal via with no ground return as an unresolved island, so a
   route with clean connectivity could not be promoted. A missing return via is
   the existing `W-SYNTH-DRC-016` SI warning, so it is now advisory; an untied
   copper island still blocks. The regression test is
   `an_unreturned_signal_via_is_advisory_not_blocking`.
8. **Source ground fence ignored the sidecar outline (fixed, historical).** On
   the earlier notched layout the fence vias were placed against the fitted
   rectangle, so three landed on or outside the notch edge (0.0/0.26 mm
   copper-to-edge). The fence now follows an `[[outline]]` path and skips points
   outside it; the final rectangular board has no custom outline and no
   edge-clearance error.

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
`source_revision` names the source commit; the later artifact commit only adds
the regenerated files. All other reports are as the tools wrote them.
`SHA256SUMS` covers all deliverables.

The failed attempts described above did **not** route clean; the committed
placement did. I did not run the final inputs three times to claim a
byte-identical board, so the committed board is one successful run, not a
proven reproducible one. FreeRouting 2.4.1 and KiCad 10.0.7 on another machine
may produce a different route. `check.sh` is the repeatable acceptance check;
it re-routes from the sources and fails on any DRC error, unconnected item, or
ground violation.
