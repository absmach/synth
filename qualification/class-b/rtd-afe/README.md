# PT100 4-wire RTD analog front end (SYN-363)

Status: design, placement and a FreeRouting route that passes Synth's routing and
DRC gates. The board is **not fabricated and not tested on hardware**. It is also
not ready to fabricate: the 12 MHz crystal part has no reviewer, no part has an
independent review, and the analog accuracy has not been measured. The evidence
and the open items are in [QUALIFICATION.md](QUALIFICATION.md).

Intended use: a bench or lab 4-wire PT100 front end with a USB readout. It is a
demonstration of a split-ground mixed-signal board, not an instrument. Do not
use it for a product or a safety function until the review items in
QUALIFICATION.md are closed.

## What it is

- 4-wire PT100 input on J2 (1x4, 2.54 mm): pin 1 F+, pin 2 S+, pin 3 S-, pin 4
  F-. The force pair carries the excitation current; the sense pair reads the
  element.
- Input filter: R_S1/R_S2 1k 0.1 %, C_D 100 nF C0G across the pair, C_CM1/C_CM2
  10 nF C0G to AGND, C_F 10 nF C0G on F+.
- U1 TI ADS124S08 24-bit delta-sigma ADC (VQFN-32, exposed pad) with the
  internal PGA and IDAC1. AIN0 is the force output, AIN1/AIN2 the differential
  sense input. The firmware is expected to select 1 mA excitation.
- U3 TI REF5025AIDR 2.5 V reference on REFP0/REFN0. C_RI and C_RV are 1 uF.
- U2 Raspberry Pi RP2350A (QFN-60), U4 W25Q128JV QSPI flash, Y1 ECS-120-20-33-TR
  12 MHz crystal with 33 pF C0G load caps, J3 3-pin SWD header, RUN pull-up.
- J1 USB-C receptacle (HRO TYPE-C-31-M-12), 5.1k CC1/CC2, 27R series on D+/D-,
  D1/D2 SOD-323 TVS on D+/D-. U5 TI LP5907-3.3 LDO (250 mA) makes +3V3 from
  VBUS.
- L2 600 ohm at 100 MHz ferrite bead isolates +3V3A for U1 and U3.
- Four M2.5 non-plated mounting holes at (6, 4), (49, 4), (6, 40) and
  (49, 40) mm.
- 50.1 x 44.1 mm, 4 layers, 1.6 mm. Board source `rtd_afe.synth`, placement
  sidecar `rtd_afe.placement.layout.toml` (board millimetres). The schematic has
  no sidecar; the compiler lays it out by group.

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
error. All filter capacitors are C0G. The `.synth` notes say "about 720 Hz"; the
difference is only the equivalent capacitance assumed there (110 nF instead of
105 nF). The ADC's digital filter and the chosen output data rate are firmware
settings and are not in this repository.

Excitation and common mode: IDAC1 sources 1 mA into AIN0/F+, through the PT100,
out of F- and through R_BIAS 1k to AGND. R_BIAS lifts the RTD common mode to
about 1.0 V plus the RTD drop (about 1.1 V at 100 ohm, 1.25 V at 250 ohm). The
gain-dependent PGA common-mode limit was not re-checked against SBAS660C in this
work; see QUALIFICATION.md.

## Grounds

AGND and DGND are separate pours on all four copper layers. AGND is the higher
priority pour where the two outlines overlap; the boundary runs between the
analog column and the MCU. The ADC's AVSS, AVSS_SW, REFN0, REFCOM, exposed pad,
DGND pin (14) and CLK pin (17) return to AGND; the MCU, flash, USB and LDO return
to DGND. R_TIE (0R, 0603) at (31.4, 31.5) mm is the only component with pads on
both nets; its AGND pad sits on the analog side of the generated bisector and
its DGND pad on the digital side. The check script proves with pcbnew that it is
the only such footprint; because a track, via or zone carries exactly one net,
no copper item can bridge the split.

## Placement and routing

Status: routed with FreeRouting 2.4.1 on Java 25, KiCad 10.0.7, Synth at the
worktree this directory was exported from. The route is not reviewed.

- Analog left, digital right. The analog block is J2 and the RC filter at the
  bottom-left, U1 at (20, 29) mm, U3 at (11, 29) mm and L2 at (30.5, 24.5) mm.
  The digital core is U2 at (38, 27) mm, U4 at (33, 18.8) mm and Y1 at
  (37, 36.5) mm. J1 is at (39.5, 6.0) mm on the top edge, U5 at (47.5, 17) mm
  and J3 at (41.5, 41.5) mm on the bottom edge.
- J2 and J3 are both at y = 41.5 mm; their bodies end 0.8 mm from the bottom
  edge. J1 is at (39.5, 6.0) mm on the top edge, 12 mm right of the board
  centre. Centering J1 was attempted; every clean-route candidate left an
  AGND or DGND pour sliver at U2's 0.4 mm pad row that the island repair could
  not stitch, so the committed board keeps J1 over the digital cluster.
- The RTD/input nets and VREF are on F.Cu only, with no layer change. QSPI also
  stays on F.Cu. The SPI, SWD, USB_DN and XIN nets change layers; see the SI
  warnings below.
- 339 track segments, 64 vias (19 on AGND, 25 on DGND, 20 signal), 586.6 mm of
  copper. 41 nets, 0 open. The router records 60 vias; the other four are
  source ground stitching.
- `kicad-cli pcb drc --severity-all --refill-zones`: 0 errors, 0 warnings, 0
  unconnected items. `pcbnew.LoadBoard` reopens the board. The AGND/DGND tie
  check passes with R_TIE as the only bridge.
- Synth's signal-integrity pass: 21 non-blocking warnings, 1 crosstalk finding
  (USB_DN/VBUS) and 20 layer changes with no ground-return via within 0.64 mm.
  The flagged nets are SPI, SWD, USB_DN, XIN, RUN, ADC_DRDY and REFOUT; no RTD,
  AIN or VREF net is flagged. See QUALIFICATION.md.
- Silkscreen: 0 DRC warnings. U1 has a hard label override at (24, 34.5) mm;
  24 passives and both TVS diodes have their reference label hidden because the
  compact refdes field has no clear spot. Their refdes remain in the BOM, PnP and
  schematic; the full list is in QUALIFICATION.md.

## Checks

From the repository root:

```
cargo build -p synth-cli
qualification/class-b/rtd-afe/check.sh
```

`check.sh` exports and autoroutes the board into `kicad/`, then runs
`kicad-cli pcb drc --severity-all --refill-zones` and `kicad-cli sch erc` on the
result, renders the 3D views, and loads the board in pcbnew to check the tie.
It needs `tools/freerouting/freerouting-2.4.1.jar` and `tools/jre25/` (both
git-excluded; symlink them from a configured Synth worktree).

Current result:

| Check | Result |
| --- | --- |
| KiCad DRC (errors / warnings / unconnected) | 0 / 0 / 0 |
| pcbnew reopen | OK |
| AGND/DGND tie | R_TIE only |
| KiCad ERC (errors / warnings) | 0 / 2 |
| Synth routing state | routed, 0 open nets |
| Synth release | `production_status: untrusted`, `release_ready: false` (unverified crystal override; not a fab approval) |
| Synth SI warnings | 21, none blocking |

The 2 ERC warnings are library-comparison warnings, not connectivity errors:
the registry uses the old name `Device:Ferrite_Bead` (KiCad 10's library has
`Device:FerriteBead`), and the embedded REF5025ID symbol differs from the
installed library copy. The exported schematic embeds its own symbols and the
netlist/PCB parity is clean.

## Committed artifacts

- `kicad/`: routed PCB, schematic, symbol library, project, DRC rules, BOM, PnP,
  Gerbers and drill files.
- `reports/`: `release.json`, the delivered DRC and ERC reports, the KiCad
  netlist, the FreeRouting routing record and the connectivity report.
- `images/`: 3D top, bottom and angled views and the schematic
  (`kicad-cli pcb render` and `kicad-cli sch export pdf`).
- `rtd_afe.synth`, `rtd_afe.placement.layout.toml`: the design sources.
- `check.sh`: the acceptance check above.
- `SHA256SUMS`: hashes of the artifacts.

The reports are as the commands produce them, except that the absolute paths of
this machine were cut to repository-relative ones in `release.json`,
`rtd_afe.routing.json` and `rtd_afe.net`. `release.json` is Synth's own format
(`synth.release.v1`); its `gate_scope` says it is a router/DRC gate, not a
fabrication approval or a component review.

## Known limits and gaps

- No VBUS fuse, TVS or reverse-polarity protection. D1/D2 protect only D+/D-.
  CC1/CC2 have no TVS. The USB shield is tied directly to DGND.
- D1/D2 are the generic `esd_usb` part: no manufacturer part number or
  datasheet. The BOM's `ESD` value is a placeholder.
- The BOM carries values, not dielectric or tolerance. The C0G filter caps and
  the 0.1 % resistors are properties of the `.synth` source; an assembler that
  reads only `bom.csv` would not see them.
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
- The board is 50.1 x 44.1 mm, compacted from the earlier 63.1 x 51.1 mm. The
  arranged floorplan groups the analog chain at the left, the MCU/flash/crystal
  and USB at the right, and puts the USB-C, PT100 and SWD connectors on the
  edges. It is one successful FreeRouting run, not a proven densest layout.
- 24 passives and D1/D2 have their reference label hidden because the refdes
  field has no clear spot in the compact layout. The refdes are still in the
  BOM, PnP and schematic; the full list is in QUALIFICATION.md.
- The mounting holes are 3.5 mm from the left edge, 3.6 mm from the right (1.2
  to 1.3 mm of material around a 2.5 mm hole), 4.0 mm from the top and 4.1 mm
  from the bottom. The remaining asymmetry is a mechanical check item.
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
- Any part was bought, measured or reviewed independently.
