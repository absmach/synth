# Qualification record: PT100 4-wire RTD analog front end (SYN-363)

This file records what was measured on the exported board. It is not a
qualification sign-off. `reports/release.json`'s `release_ready` and
`production_status` cover only Synth's router, DRC and signal-integrity gates;
its own `gate_scope` says so. Every review gate below that needs a human is
still open.

The board is not fabricated, not powered, not measured and not independently
reviewed. Do not call it proven, clean or production-ready.

## Board and stackup

- Outline: 72.0 x 56.0 mm. Edge.Cuts runs x 0.55 to 72.54 mm,
  y -0.21 to 55.78 mm (71.99 x 55.99 mm).
- Four copper layers, 1.6 mm: F.Cu, In1.Cu, In2.Cu, B.Cu. There is no
  explicit stackup block in the exported board; the fab house must choose the
  prepreg/core. No controlled-impedance stackup was requested.
- AGND and DGND pours are declared on all four layers. AGND has priority 2,
  DGND priority 1, so AGND wins where the two zone outlines overlap. The filled
  boundary runs between the analog column and the MCU. The four mounting holes
  have 5.6 x 5.6 mm all-layer rule areas around them.
- 54 footprints, 41 nets. The analog column (J2, the RC filter, U1, U3, L2) is
  on the left; the MCU, flash, crystal, USB-C and LDO are on the right.

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
| AGND/DGND tie | R_TIE is the only footprint with pads on both nets | `check.sh` asserts it; see below |
| KiCad ERC | 0 errors, 2 warnings | `reports/rtd_afe.kicad-erc.json` |
| Synth routing | routed, 41 connected nets, 0 open | `reports/rtd_afe.routing.json` |
| Synth release | `production_status: untrusted`, `release_ready: false` | `reports/release.json`; the unverified-crystal override, not a DRC failure |
| Synth signal integrity | 29 warnings, 0 blocking | `reports/release.json` |

The delivered board is `kicad/rtd_afe.kicad_pcb`
(`cec1f31f2575c8935d8203ef092b1129bfcd016ca8fb8461ddbe47b55e8c4ce6`).
The schematic is `kicad/rtd_afe.kicad_sch`
(`e81a9a0ebf18d5fd93cef8b224df16134033824da6402abbb8f845048dbd89c4`).

### DRC history

The first export in this worktree (before the label overrides and the UUID
fix below) reported 0 errors, 0 unconnected items and 12 warnings: three
`silk_overlap` and nine `silk_over_copper`. All were reference labels:
C_DV1's label sat on U2's bottom pads; R_AV's label sat on C_VI; C_VI's label
sat on C_DVB. The delivered board has the labels moved with hard sidecar
overrides (`C_DV1` vertical along U2's right edge, `R_AV` up-right of R_AV,
`C_VI` right of C_VI). `kicad-cli` DRC on the delivered board reports none.

### The AGND island run

One intermediate run, with a 1 mm move of C_IO2 that was later reverted, ended
with 0 errors, 0 warnings and 1 unconnected item: the F.Cu and B.Cu AGND zones
were listed against each other, with an island near (33.84, 25.25) mm and no
0.6 mm spot for a stitching via. That run is not committed and its artifacts
were overwritten. The committed placement is the one that routed clean in
8:33. This is a reproducibility caveat, not a board defect: FreeRouting's
result and Synth's ground stitching are sensitive to placement. `check.sh`
re-routes from scratch and asserts the result; it is the same check that
failed on the intermediate placement.

## ERC findings and dispositions

`reports/rtd_afe.kicad-erc.json` has two warnings and no errors:

| Warning | Disposition |
| --- | --- |
| `lib_symbol_issues`: Symbol `Ferrite_Bead` not found in `Device` | The registry part uses `Device:Ferrite_Bead`; KiCad 10's library has `Device:FerriteBead`. The schematic embeds its own symbol and the netlist is correct. The registry data is stale; not changed here to keep this change set to this board. |
| `lib_symbol_mismatch`: Symbol `REF5025ID` does not match the copy in `Reference_Voltage` | The embedded symbol is generated from the registry pins, so its drawing differs from the installed library copy. Pin numbers and electrical types match the reviewed part; the netlist and PCB parity are clean. Cosmetic. |

Both are library-comparison warnings from an embedded-symbol export, not
connectivity findings. A human should confirm that disposition.

## Synth signal-integrity warnings

`reports/release.json` lists 29 `W-SYNTH-DRC-016` findings: a layer change with
no ground-return via within 0.64 mm. None is blocking. They are on:
REFOUT (2), SPI_MOSI (2), SPI_CS (2), SPI_SCK (2), SPI_MISO (2), ADC_DRDY (2),
ADC_START (2), XOUT_R (2), RUN (1), QSPI_SS (2), QSPI_SCLK (2), QSPI_SD0 (2),
QSPI_SD2 (2), USB_DP_C (2), USB_DN_C (2). No RTD_FP/RTD_SP/RTD_SN/RTD_FN,
AIN1, AIN2 or VREF finding appears. The six RTD/input nets and VREF are on
F.Cu only; see the measured lengths below.

The warnings still matter for the digital interface: the SPI (to 16.1 mm,
0.5 mm pitch ADC), QSPI and full-speed USB nets change reference layer, and
on this split-ground board the return current has to cross from an AGND-referenced
region to a DGND-referenced one. A return via next to each layer change would
help; this was not added, because it is a layout change that needs an SI review
first.

## Synth validate warnings

`target/debug/synth validate` (with an empty `XDG_DATA_HOME`) also emits
warnings. They are not part of the DRC acceptance bar, but they are not hidden:

| Finding | Disposition |
| --- | --- |
| `E-SYNTH-POWER-004` on VBUS: 7 load endpoints, 1 capacitor | Heuristic. C_L1 (2.2 uF) is the LDO input capacitor. There is no separate VBUS bulk capacitor, fuse or TVS; that is a real gap, see the USB section. |
| `W-SYNTH-ANOMALY-001` | Statistical topology heuristic (score -2.43 against a 0 boundary); no action. |
| `W-SYNTH-SUPPLY-002` x10 | Generic parts (passives, headers, ESD) have no distributor identity. |
| `W-SYNTH-PART-UNVERIFIED` | Y1, the 12 MHz crystal, has no reviewer. This is the release override. |
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
| J1 | HRO TYPE-C-31-M-12 (USB-C) | None in the registry; LCSC C165948 | 16 signal/power pins plus 4 shield pads: A1/A12/B1/B12/SH DGND, A4/A9/B4/B9 VBUS, A5 CC1, B5 CC2, A6/B6 DP, A7/B7 DN; SBU1/SBU2 open | 5.1k CC1/CC2; 27R DP/DN; D1/D2 TVS | Not reviewed |
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
IOVDD 4 x 100 nF and DVDD 2 x 100 nF; W25Q128 VCC 100 nF.

The BOM (`kicad/bom.csv`) carries values only. `dielectric "C0G"`, voltage and
the 0.1 % tolerance are properties of `rtd_afe.synth`; an assembler reading only
the BOM would not see them. This is an exporter limitation, not a source
omission; see "Left to a human".

## AGND/DGND tie argument

The design requirement is one and only one connection between AGND and DGND.
The delivered board satisfies it as follows:

1. AGND and DGND are separate zones: eight zone objects, one per net per copper
   layer, with AGND priority 2 and DGND priority 1. Each zone has one net.
2. Every track and via carries one net. A zone fill belongs to one zone and
   therefore one net. No copper item can be on both nets.
3. Only a component can join them. `check.sh` loads the board in pcbnew and
   selects footprints whose pads include both `AGND` and `DGND`; the assertion
   requires exactly `R_TIE`. On the delivered board it prints
   `reopen ok; AGND/DGND tied only at R_TIE`.
4. R_TIE is a 0R 0603 at (41.5, 36.5) mm, on the pour boundary between the ADC
   side and the MCU side. Its AGND pad reaches the AGND pour and its DGND pad
   reaches the DGND pour. It is the only such component; removing it leaves the
   two grounds open.

Why this split: the ADC's digital return (DGND pin 14), CLK, AVSS, AVSS_SW,
REFN0, REFCOM and exposed pad are tied to AGND, so the ADC's own return current
stays on the analog side and does not flow under the MCU. The MCU, flash, USB
and LDO return to DGND. The single tie is a DC/low-frequency reference; a 0R
0603 has some inductance, so the tie is not an RF short. No impedance or
ground-noise measurement was made. This is the main item for the analog/SI
review.

The `.synth` note says R_TIE is "next to U1". It is at the pour boundary about
15 mm from U1's centre, near the SPI crossing. The note's wording is loose; the
coordinates above are the measured ones.

## Measured layout numbers

- 354 track segments, 77 vias: 22 AGND, 26 DGND, 29 signal. 732.5 mm of track.
- 41 nets, 0 open. All AGND/DGND pads are in one cluster per net (the DRC has no
  unconnected items; the tie check above covers the split).
- Analog nets, all F.Cu only, no via:

| Net | Length |
| --- | --- |
| RTD_FP | 25.23 mm |
| RTD_SP | 6.98 mm |
| AIN1 | 19.78 mm |
| RTD_SN | 6.79 mm |
| AIN2 | 27.78 mm |
| RTD_FN | 4.17 mm |
| VREF | 10.64 mm |

- SPI_SCK is 16.14 mm with 2 vias; SPI_MOSI/MISO/CS 16.4/16.6/15.6 mm with
  2 vias each. QSPI_SS/SCLK/SD0..SD3 are 11.8 to 18.6 mm.
- USB: the RP2350-side nets (after the 27R) are 8.28 mm (DP) and 12.23 mm (DN);
  the connector-side nets (before the 27R) are 21.63 mm each. Total path
  29.91 mm (DP) and 33.86 mm (DN), 2 vias each, full speed. The pair is not
  routed as a coupled pair and has no controlled impedance.
- Board outline and stackup above; 54 footprints.

## Left to a human

Nothing below is signed off. The list is deliberately blunt.

- **SI/PI/RF expert**: review the 24-bit analog layout, the AGND/DGND split and
  its single tie, the ADC reference decoupling, the RC filter against the chosen
  ADC data rate, the IDAC output's 10 nF load and settling, the PGA
  common-mode range at the chosen gain, the unused-input termination, and the
  29 layer-change warnings on the digital nets. None of these were simulated or
  measured.
- **Metrology/analog**: the measurement is not ratiometric. The IDAC's absolute
  accuracy and drift, the REF5025 grade and drift, the 1 k filter resistors and
  the PT100 wiring resistance set the accuracy; there is no calibration or
  error budget. A 4-wire measurement with 0.1 % resistors is not automatically a
  0.1 % thermometer.
- **USB/ESD**: no VBUS fuse, TVS or reverse-polarity protection; CC1/CC2 have no
  TVS; the shield is tied directly to DGND; D1/D2 have no MPN. USB-C and ESD
  compliance were not checked.
- **Mechanical**: the bottom mounting holes are 2.8 mm from the cut edge
  (1.5 mm of material around a 2.5 mm hole) while the top pair is 4.2 mm from
  the edge; the enclosure, screw-head clearance, connector overhang and header
  orientation were not checked. Synth places no board-name or revision silk.
- **Fabrication/DFM**: no fab house has seen the Gerbers, drill, BOM or PnP.
  The 0.4 mm QFN-60, the 0.5 mm VQFN-32 and the 0.5 mm USB-C need a fab
  capability check; the four-layer stackup is the default, not a controlled
  impedance quote. Assembly of the generic passives needs the C0G/tolerance
  attributes from the source, not just the BOM values.
- **Firmware**: the RP2350 boot, QSPI flash, ADC gain/data-rate/IDAC registers,
  USB descriptors and the PT100 conversion/calibration do not exist in this
  repository. Nothing was powered.
- **Independent sign-off**: no part reviewer, no board reviewer and no
  second-person check of this record. The only automated checks are the datasheet
  pin tests, the KiCad DRC/ERC and the tie assertion.

## Synth defects hit in this work

1. **Duplicate footprint-body UUIDs (fixed).** Inlining a KiCad 10 footprint
   body kept the library's child UUIDs, and `ensure_body_uuids` only filled
   missing ones and keyed them by `lib_id`, not by the placed instance. Every
   0603 shared its pads' and silk lines' UUIDs: 79 duplicated ids, one shared by
   23 footprints. KiCad keys board items by UUID, so `kicad-cli pcb drc`
   attributed a silk collision to an item 11 mm away (R_AV's label against
   C_L2's silk). The fix gives each instance its own deterministic ids and
   replaces stock ones; the regression test is
   `inlined_body_blocks_carry_unique_deterministic_uuids`. After the fix the
   board has 1094 items, 1094 unique UUIDs, and the DRC warnings name the real
   neighbours. This is what made the zero-warning result trustworthy.
2. **Ground stitching is placement-sensitive (not fixed).** An intermediate
   placement produced an unconnected AGND F.Cu/B.Cu pair with no legal 0.6 mm
   stitching-via spot. A different placement routed clean. The check now catches
   it (it failed the run), but the stitching search is not robust to a crowded
   pour. The committed placement is the one that passed; a future placement
   change can bring the island back.
3. **BOM omits component attributes (not fixed).** `bom.csv` has value, kind,
   symbol, footprint, LCSC and MPN, but not `dielectric`, `voltage` or
   `tolerance`, which are present in the `.synth` source. For this board that
   hides the C0G filter requirement and the 0.1 % resistors from an assembler.
4. **No free-text silkscreen (not fixed).** The board name, revision and header
   pin functions cannot be put on the silk; only reference labels exist.

## Reproducibility

Inputs and outputs of the committed run:

| File | SHA-256 |
| --- | --- |
| `rtd_afe.synth` | `c7641ede7eeef4a5f09324f46160bc0b91eed6e3310a25f005b520d91d5facf1` |
| `rtd_afe.placement.layout.toml` | `28a65895b8e527912ea253ff7f9c28958a3fed54f610e1c249e6f8724aac4797` |
| `kicad/rtd_afe.kicad_pcb` | `cec1f31f2575c8935d8203ef092b1129bfcd016ca8fb8461ddbe47b55e8c4ce6` |
| `kicad/rtd_afe.kicad_sch` | `e81a9a0ebf18d5fd93cef8b224df16134033824da6402abbb8f845048dbd89c4` |
| `reports/rtd_afe.kicad-drc.json` | `8ffd5aaae159d772b20d856694a3e808d93ab671bfbeab521a591e96beb305b0` |
| `reports/rtd_afe.kicad-erc.json` | `db8491b2bbe011af24ded22a23cfbc60094eee6852a6a4484b238c147f91501b` |
| `reports/release.json` | `6351c2c16c09311c015f4573e951b8623b9a45f688d5ea672c8bf59bfc36bec4` |
| `reports/rtd_afe.routing.json` | `da04e647f0d3e837d4ff52cebdd6e1010a78aae02ff0be9790ca7522b47812fa` |

The release, routing and netlist reports had the absolute paths of the machine
that produced them cut to repository-relative ones, matching the usb-uart
board's convention; `source_revision` is null because Synth did not stamp it.
All other reports are as the tools wrote them. `SHA256SUMS` covers the rest of
the deliverables.

The intermediate placement above did **not** route clean; the committed
placement did. I did not run the final inputs three times to claim a
byte-identical board, so the committed board is one successful run, not a
proven reproducible one. FreeRouting 2.4.1 and KiCad 10.0.7 on another machine
may produce a different route. `check.sh` is the repeatable acceptance check;
it re-routes from the sources and fails on any DRC error, unconnected item, or
tie violation.
