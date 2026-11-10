# PT100 4-wire RTD analog front end (SYN-363)

Status: design, placement and a FreeRouting route that is **untrusted and not
release-ready**. The committed evidence is historical (source revision
`01a77d54`); on default settings the export ends `validation_failed` with 12
blocking return-via findings. The board is **not fabricated and not tested on
hardware**. The
SI/PI/RF review is being carried out by the designated reviewer; the automated
evidence and the open items are in [QUALIFICATION.md](QUALIFICATION.md). The
analog accuracy has not been measured, and several sourcing and decoupling items
are open (see Known limits and gaps).

Intended use: a bench or lab 4-wire PT100 front end with a USB readout. It is a
compact mixed-signal board on a single ground plane, not an instrument. Do not
use it for a product or a safety function until the review items in
QUALIFICATION.md are closed.

## What it is

- 4-wire PT100 input on J2 (1x4, 2.54 mm): pin 1 F+, pin 2 S+, pin 3 S-, pin 4
  F-. The force pair carries the excitation current; the sense pair reads the
  element.
- Input filter: R_S1/R_S2 1k 0.1 %, C_D 100 nF X7R across the pair, C_CM1/C_CM2
  10 nF C0G to GND, C_F 10 nF C0G on F+.
- U1 TI ADS124S08 24-bit delta-sigma ADC (VQFN-32, exposed pad) with the
  internal PGA and IDAC1. AIN0 is the force output, AIN1/AIN2 the differential
  sense input. The firmware is expected to select 1 mA excitation.
- U3 TI REF5025AIDR 2.5 V reference on REFP0/REFN0. C_RI and C_RV are 1 uF.
- U2 Raspberry Pi RP2350A (QFN-60), U4 W25Q128JVSIQ QSPI flash (SOIC-8 208 mil, registry `w25q128_flash_208mil`), Y1 ECS-120-20-33-TR
  12 MHz crystal with 33 pF C0G load caps, J3 3-pin SWD header, RUN pull-up.
- J1 USB-C receptacle (HRO TYPE-C-31-M-12), 5.1k CC1/CC2, 27R series on D+/D-,
  D1/D2 SOD-323 TVS on D+/D-. U5 TI LP5907-3.3 LDO (250 mA) makes +3V3 from
  VBUS.
- L2 600 ohm at 100 MHz ferrite bead isolates +3V3A for U1 and U3.
- Four M2.5 non-plated mounting holes at (6, 3.5), (39, 3.5), (6, 40.7) and
  (39, 40.7) mm.
- 40.1 x 44.2 mm rectangular board, 1772 mm², 4 layers, 1.586 mm declared
  stackup (1.6 mm nominal FR4). Board source `rtd_afe.synth`, placement sidecar
  `rtd_afe.placement.layout.toml` (board millimetres). The schematic has no
  sidecar; the compiler lays it out by group.

## Signal chain

J2 -> R_S1/R_S2 and the C_D/C_CM/C_F network -> U1 AIN1/AIN2. The reference is
absolute: VREF = 2.5 V from U3. The current is not used as the reference, so the
IDAC's absolute accuracy and drift limit the measurement unless the firmware
calibrates them. That trade is deliberate and is called out as an open accuracy
item in QUALIFICATION.md.

Nominal filter corners (this work, assuming the 2 k filter resistance dominates
the 100 to 250 ohm PT100):

| Path | Elements | Corner |
| --- | --- | --- |
| Differential | R_S1 + R_S2 = 2 k, C_D 100 nF + C_CM1/C_CM2 in series (5 nF) = 105 nF | about 758 Hz |
| Common mode | R_S 1 k, C_CM 10 nF | about 15.9 kHz |

C_D is ten times each C_CM, so capacitor mismatch turns into little differential
error. C_CM and C_F are C0G; C_D is X7R because no 0603 C0G 100 nF is stocked. The `.synth` notes say "about 720 Hz"; the
difference is only the equivalent capacitance assumed there (110 nF instead of
105 nF). The ADC's digital filter and the chosen output data rate are firmware
settings and are not in this repository.

Excitation and common mode: IDAC1 sources 1 mA into AIN0/F+, through the PT100,
out of F- and through R_BIAS 1k to GND. R_BIAS lifts the RTD common mode to
about 1.0 V plus the RTD drop (about 1.1 V at 100 ohm, 1.25 V at 250 ohm). The
gain-dependent PGA common-mode limit was not re-checked against SBAS660C in this
work; see QUALIFICATION.md.

## Ground

One GND plane on all four copper layers. There is no AGND/DGND split and no tie
resistor. The ADC's AVSS, AVSS_SW, REFN0, REFCOM, exposed pad, DGND pin (14) and
CLK pin (17) return to the same plane as the MCU, flash, USB and LDO. `check.sh`
asserts with pcbnew that the board has `GND` and no `AGND` or `DGND` net. Issue
#363 asked for an AGND/DGND split; the issue owner has not signed off on this
deviation. The return-via advisories on the digital nets are listed in
QUALIFICATION.md for the SI review.

## Placement and routing

Status: routed with FreeRouting 2.4.1 on Java 25, KiCad 10.0.7, Synth at the
worktree this directory was exported from (source revision 01a77d54). The board
was produced with this branch's opt-in fine-pitch return-via relaxation
(`--fine-pitch-return-via-advisory`, needs SI-review sign-off, not given). On
default settings it ends `validation_failed` with 12 blocking return-via
findings, so the board is untrusted and not release-ready (see
QUALIFICATION.md). The route is under review by the designated reviewer.

- Analog left, digital right, packed rather than spread. The input chain is J2
  at (14.8, 41.7) mm, the RC filter just above it and U1 at (12, 25.1) mm. The
  reference and support parts sit in the top-left: U3 at (10.5, 11.6) mm, C_RI
  at (4.8, 11.3) mm, L2 at (13, 6.5) mm, C_RO at (16, 13.6) mm and R_RST at
  (19.8, 14) mm, with R_CC2 at (21, 11.5) mm. The digital core is U2 at
  (28, 29.6) mm, U4 at (22, 19) mm and Y1 at (28, 36.3) mm. U5 is at
  (36.5, 10.5) mm and J3 at (28, 41.7) mm on the bottom edge.
- The outline is fitted to the placement: 40.1 x 44.2 mm, 1772 mm². The largest
  bare rectangle was 7 x 8 mm (56 mm²) on the earlier route (not re-measured after the C_IO, L1 and U4 moves), down from 19 x 19 mm (361 mm²) on the
  50.1 x 44.1 mm rectangle. The board is not a scaled copy: the groups were
  re-packed, so the RTD and AIN lengths changed (RTD_FP 16.78 mm, AIN1
  12.39 mm, AIN2 13.55 mm) and VREF is 37.85 mm of track on F.Cu in the current
  route, still with C_RV at REFP0.
- J2 and J3 are both at y = 41.7 mm; their bodies end about 0.71 mm from the
  bottom edge. J1 is at (22.55, 6.0) mm, centred on the 40.1 mm top edge and
  1.27 mm from it. The extra 1 mm of height over the first compact pass gives
  U2's top-row escapes room; that is what makes the centred connector route
  without an untied GND fragment.
- The sidecar carries a plain rectangular `[[outline]]` (x 2.50..42.59,
  y -0.01..44.21); there is no notch and no post-export cut.
- The RTD/input nets and VREF are on F.Cu only, with no layer change. QSPI is
  mostly on F.Cu, with SD1/SD2 changing layer. SPI_SCK/MOSI/MISO/CS, USB_DN and
  some ADC-control nets change layers; see the SI findings below.
- 389 track segments, 89 vias (60 on GND, 29 signal), 688.4 mm of track. Six of
  the vias are the compact 0.46/0.20 mm size; the rest are 0.6/0.3 mm. 40 nets,
  0 open. The router imported 71 of the vias; the others are not
  router-imported (source stitching and repair vias).
- One via sits inside a pad (C_DV.2, at (16.138, 22.835) mm). `release.json`
  lists it as a fabrication note: order with filled and capped vias.
- `kicad-cli pcb drc --severity-all --refill-zones`: 0 errors, 48 warnings (47
  `lib_footprint_mismatch`: the exported footprints differ from KiCad 10's library
  copies; 1 `silk_edge_clearance`: the C_DVB reference label is clipped by the
  board edge at (41.05, 20.5) mm), 0 unconnected items. `pcbnew.LoadBoard` reopens the board and `check.sh` confirms
  one GND plane with no AGND or DGND net.
- Synth's signal-integrity pass, with the fine-pitch relaxation on (default
  settings: 12 blocking return-via findings): 12 non-blocking findings, 11 `W-SYNTH-DRC-016`
  layer changes with no ground-return via within 0.80 mm (advisories, see
  QUALIFICATION.md) and 1 parallel-run finding (SPI_MISO and ADC_DRDY, 3.5 mm on
  B.Cu, 0.203 mm apart). No RTD, AIN or VREF net is flagged.
- Silkscreen: 1 DRC warning (the C_DVB label at the board edge, above). U1 has a hard label override at (16, 30.6) mm;
  29 passives/TVS and H1/H2/H3/J1/J3 have their reference label hidden because
  the compact refdes field has no clear spot. Their refdes remain in the BOM, PnP and
  schematic; the full list is in QUALIFICATION.md.

## Checks

From the repository root:

```
cargo build -p synth-cli
qualification/class-b/rtd-afe/check.sh
```

`check.sh` exports and autoroutes the board into `kicad/`, then runs
`kicad-cli pcb drc --severity-all --refill-zones` and `kicad-cli sch erc` on the
result, renders the 3D views, and loads the board in pcbnew to check that it has
one GND plane. The relaxation is opt-in: `check.sh` passes
`--fine-pitch-return-via-advisory` only when run with
`FINE_PITCH_RETURN_VIA_ADVISORY=1`; the committed evidence was produced with it
on (SI sign-off not given). Without it the script reports the blocking result
and does not replace the evidence. It needs `tools/freerouting/freerouting-2.4.1.jar` and
`tools/jre25/` (both git-excluded; symlink them from a configured Synth
worktree).

Current result:

| Check | Result |
| --- | --- |
| KiCad DRC (errors / warnings / unconnected) | 0 / 48 / 0 |
| pcbnew reopen | OK |
| Ground | one GND plane; no AGND or DGND net |
| KiCad ERC (errors / warnings) | 0 / 1 |
| Synth routing state | routed, 0 open nets |
| Synth release | `production_status: production`, `release_ready: true` (router/export gates only; not a fab approval, see `gate_scope`) |
| Synth SI findings | 12, none blocking (11 `W-SYNTH-DRC-016` advisories, 1 parallel run) |

The ERC warning and 47 of the 48 DRC warnings are library-comparison warnings,
not connectivity errors. The embedded REF5025ID symbol differs from the installed
library copy. The 47 are `lib_footprint_mismatch`: Synth's exporter
now redraws each footprint's reference label at 0.8 mm with an 0.18 mm stroke
and trims silkscreen that would clip a pad or via, so the embedded footprint
differs from the `Resistor_SMD`/`Capacitor_SMD` library copy. No copper, pad or
drill is changed. The exported schematic embeds its own symbols and the
netlist/PCB parity is clean. This disposition has not been reviewed by a human.

## Committed artifacts

- `kicad/`: routed PCB, schematic, symbol library, project, DRC rules, BOM, PnP,
  Gerbers and drill files.
- `reports/`: `release.json`, the delivered DRC and ERC reports, the KiCad
  netlist, the FreeRouting routing record and the connectivity report.
- `images/`: 3D top, bottom and angled views and the schematic. The 3D views
  are rendered with `kicad-cli pcb render`, the schematic with
  `kicad-cli sch export pdf` and `pdftoppm`.
- `kicad/rtd_afe.step`: board STEP with 3D models, exported by `kicad-cli pcb export step --no-dnp`; the 3D views use the same models. Stock KiCad models, except the ADS124S08 and RP2350, which use the simplified models in `agent-run/step-models/` (from datasheet dimensions, not vendor CAD), and the USB-C receptacle, which uses the GrabCAD model in `../models/` (see its README for source and offset). The STEP and the 3D images are made from a scratch copy of the board that names these models; the committed `kicad_pcb` still names KiCad's library path for the USB-C file, because the exporter has no custom-model path yet. The mounting holes have no model (the KiCad MountingHole step files are not installed).
- `rtd_afe.synth`, `rtd_afe.placement.layout.toml`: the design sources.
- `check.sh`: the acceptance check above.
- `SHA256SUMS`: hashes of the artifacts.

The committed `kicad/`, `reports/`, `images/` and `SHA256SUMS` are historical output from source revision 01a77d54; the current tree does not route, so they are not reproducible from head (see QUALIFICATION.md, Reproducibility). `reports/release.json` (production, `release_ready: true`, reviewers all `reviewed`) is generated output from that revision and does not reflect the PR status (untrusted, `release_ready: false`, crystal unreviewed). `kicad/bom.csv` is historical and predates the source pins (C_D, L1, Y1).

The reports are as the commands produce them, except that `check.sh` rewrites
the checkout path in `release.json`, `rtd_afe.routing.json` and `rtd_afe.net` to
a repository-relative one and stamps `source_revision` from the checked-out
commit. `release.json` is Synth's own format (`synth.release.v1`); its
`gate_scope` says it is a router/DRC gate, not a fabrication approval or a
component review.

## Known limits and gaps

- No VBUS fuse, TVS or reverse-polarity protection. D1/D2 protect only D+/D-.
  CC1/CC2 have no TVS. The USB shield is tied directly to GND.
- D1/D2 are pinned to ESD5V0B03-323 (LCSC C521915) in the source; the registry
  part is generic and the BOM's `ESD` value text is a placeholder.
- The BOM carries `dielectric`, `voltage`, `tolerance` and `power` columns from
  the source, so the C0G filter caps and the 0.1 % resistors are visible to an
  assembler. Empty cells mean the source does not declare that attribute.
- The source declares `requires via_in_pad` and the manufacturer profile
  `jlc-filled-via`. This route has one via inside a pad (C_DV.2), listed in
  `release.json` as a fabrication note. JLCPCB has not accepted that process.
- There are no test points. The 12-pad set (rails, reference, USB and SWD) was
  tried on the earlier outline; every routing attempt left GND islands. The
  top-left area it used now holds the reference cluster.
- No VBUS current budget was measured. The LDO is 250 mA; the RP2350 datasheet
  peak plus the ADC, reference and flash is expected to be below it, but this
  was not checked against the loads.
- The reference is absolute, not ratiometric. IDAC accuracy and drift therefore
  enter the result; no calibration or IDAC accuracy check was done.
- The unused ADC analog inputs (AIN3..AIN11, AINCOM), REFP1/REFN1, the ADC GPIOs
  and pin 25 are left open. Their termination is not covered by a datasheet
  review here.
- The firmware does not exist in this repository: gain, data rate, IDAC value,
  register map, USB descriptors and calibration are all unwritten/untested.
- Synth has no free-text silkscreen primitive, so the board name and revision
  are not on the copper. Header pin functions are not labelled on the silk.
- The board is 40.1 x 44.2 mm (1772 mm²), down from the 50.1 x 44.1 mm
  rectangle and the earlier 63.1 x 51.1 mm. The floorplan packs the analog chain
  and reference at the left and the MCU/flash/crystal and USB at the right, with
  the groups interlocking vertically instead of sitting in two sparse columns.
  It is one successful FreeRouting run, not a proven densest layout.
- The sidecar carries a plain rectangular `[[outline]]`, not a notch; the
  exporter reproduces it without a post-export cut.
- 29 passives/TVS and H1/H2/H3/J1/J3 have their reference label hidden because
  the refdes field has no clear spot in the compact layout. The refdes are still in
  the BOM, PnP and schematic; the full list is in QUALIFICATION.md.
- RP2350 decoupling is below the recommendation. The board has four IOVDD 100 nF
  caps (C_IO1..C_IO4) for six IOVDD pins, and two DVDD caps (C_DV1, C_DV2) plus
  C_DVB bulk for three DVDD pins. Raspberry Pi's hardware design guide (section
  2.2.1) recommends one 100 nF per power pin, with pins 53/54 sharing one. Four
  more caps could not be placed without breaking routing; left for the SI/PI
  review.
- L1 is pinned to Sunlord MPL2012S3R3MHT (C98331, 0805); its DCR and saturation current are not checked against the RP2350 datasheet. The Pi-recommended Abracon AOTA-B201610S3R3-101-T (C42411119) does not fit the 0805 footprint.
- C_D is X7R 0603 (C14663), not C0G: no 0603 C0G 100 nF is stocked. A 1206 C0G part (C170182) broke routing and was reverted.
- Y1 is pinned to C2449814 (exact MPN).
- Issue #363 asked for an AGND/DGND split; this board uses one GND plane and the
  issue owner has not signed off on the deviation.
- No GitHub issue numbers are linked for the Synth defects listed in
  QUALIFICATION.md.
- The mounting holes leave 2.15 to 2.24 mm of material around a 2.7 mm hole, the ISO 273 fine clearance for M2.5.
  J2/J3 are about 0.71 mm from the bottom edge and J1 1.27 mm from the top.
  The enclosure and screw-head clearance are mechanical check items; the
  measured table is in QUALIFICATION.md.
- The KiCad ERC library-name warnings remain (see above). They do not affect
  the netlist.

## Not verified

- Analog accuracy, noise, linearity, drift and the actual PT100 reading.
- The IDAC absolute accuracy/drift, PGA common-mode range at the chosen gain,
  the ADC's unused-input termination, and the RC filter against the chosen ADC
  data rate.
- The 1.1 V core regulator, the 12 MHz crystal's ESR/drive level, the flash
  QSPI timing, and the USB full-speed signal integrity.
- ESD, surge, reverse polarity and USB-C compliance.
- Mechanical fit, connector and header orientation, mounting-hole clearance.
- Any fabrication or assembly check. Nothing was sent to a fab house.
- Any part was bought or measured. Part-level registry reviews exist (see
  QUALIFICATION.md); the HRO and Tech Public datasheets were read as scans.
