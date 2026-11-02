# 24 V in, 3.3 V out buck with input parts (PTC, reverse-block diode, TVS) (SYN-67)

Status: designed, placed by a hand-pinned sidecar, routed, DRC and ERC counted, Gerber and drill files
generated. Not fabricated, not powered, no hardware test. Both parts that carry registry review records
are signed: `lmr38010` and `ltst_c190kgkt` were reviewed by Steve Munene on 2026-10-29, and the fab package
reports `release_ready: true`. The review evidence for both is below.
Intended for 24 V +10% in and 1 A out. The converter is rated 1 A, and the PTC and the input connector are
further limits (see Limits). No surge test was run. The TVS clamp (48.4 V at 12.4 A) is below the
converter's 85 V absolute maximum; a sustained overvoltage is limited by the PTC and the 50 V capacitors.

## Purpose

Ticket #67 under #38 (Class B qualification): a switching-regulator board with input and output filtering,
feedback, power-good and thermal copper, plus a negative variant that the feedback-safety rule
(E-SYNTH-POWER-011, PR #56) must flag.

## What

- `buck.synth`, negative `buck_neg_fb_sw.synth`, the placement sidecar `buck.placement.layout.toml`, hashes in `SHA256SUMS`, matrix and `#38` traceability in `QUALIFICATION.md`, evidence in `kicad/`, `reports/`, `images/`.
- `kicad/`: the KiCad project, BOM, pick-and-place, `gerbers/` (22 Gerbers plus the job file, 4 copper layers) and `drill/` (PTH and NPTH). `reports/`: `release.json` (source/DRC gate) and `release-fab.json` (fab package, untrusted), DRC, ERC, routing and connectivity.
- Chain: J1 (JST XH) -> F1 PTC -> D1 SS36 Schottky (reverse block) -> VIN node.
  VIN node: D2 SMBJ30A TVS, C1 47 uF bulk, C2 10 uF, C3 220 nF (on the U1 VIN pin), U1 LMR38010, UVLO divider.
- U1 -> L1 6.8 uH -> C5, C6, C9 (3x 22 uF, 25 V X7R) -> J2. C4 100 nF boot. R_RT 33k from RT/SYNC to GND sets 800 kHz. U1 has no VCC pin, so there is no VCC capacitor.
- U1 pins: 1 GND, 2 EN, 3 VIN, 4 RT/SYNC, 5 FB, 6 PG, 7 BOOT, 8 SW, 9 exposed pad.
- Feedback: R_FB_T 100k from the output node (L1 pad 2 side), R_FB_B 43.2k, C7 56 pF across R_FB_T.
  Vout = 1.0 x (1 + 100k/43.2k) = 3.315 V typical, 3.26 to 3.36 V over Vref 0.985 to 1.015 V (ideal resistors).
- Power-good: U1 pin 6 is open drain, pulled up to +3V3 through R_PG 100k and brought out on J2 pin 3. D4 (green, "PG FAULT") and R_PGLED 470 R run from +3V3 to PG, so the LED lights while PG is low and stays dark while power is good.
- Thermal: U1 pin 9 is a 2.41 x 3.1 mm exposed pad soldered to the F.Cu GND pour, with a 2 x 3 array of six GND vias in the pad (see Thermal). The board is `manufacturer "jlc-filled-via"` with `requires via_in_pad` (resin-filled and capped vias).
- Negative file: identical except R_FB_T and C7 hang from SW. It emits E-SYNTH-POWER-011 and the same 14 warnings as the good board.
- Board: 70.99 x 33.99 mm (2413 mm2, Edge.Cuts line centres), 4 layers, GND pours on all layers. Power tracks 0.5 mm, signal 0.127 mm.
- Two M2.5 non-plated mounting holes (H1, H2) in opposite corners.

## Why LMR38010 and not LMR33640

The first revision used the LMR33640 (38 V absolute maximum). A review found that the SMBJ30A clamps at 48.4 V
(12.4 A), above that maximum, so the TVS could not protect it. The LMR38010 is rated 85 V absolute maximum
(SW 85 V too), above the clamp. The change also removed the VCC pin and its capacitor, moved PG to pin 6
and VIN to pin 3, and added R_RT.

## UVLO (LMR38010 SNVSBM3B, 9.2.2.8, Eq 13 and 14)

Datasheet values: VEN-H (rising) 1.1 / 1.25 / 1.4 V and VEN-L (falling) 0.95 / 1.10 / 1.22 V (min/typ/max).
R_UV_B = 100k, R_UV_T = 1.37M, ratio 1 + 13.7 = 14.7.
- Start = 1.25 x 14.7 = 18.4 V typical (16.2 to 20.6 V over the VEN-H range).
- Stop = 1.10 x 14.7 = 16.2 V typical (14.0 to 17.9 V over the VEN-L range).
- The min and max of the two ranges are not independent, so the extreme pairs are not a guaranteed hysteresis. 1% resistors add about 1% to each figure.
- EN at 28 V in is 28 x 0.1/1.47 = 1.90 V; the EN absolute maximum is VIN + 0.3 V.

## Frequency and inductor

- R_RT 33k gives 800 kHz (datasheet RT versus frequency table under Figure 8-4; Table 8-1 lists 750 kHz at 34.8k and 1000 kHz at 25.5k). The RT/SYNC pin must not float (8.3.4).
- L1 6.8 uH: ripple (26.4 - 3.3) x (3.3/26.4) / (6.8 uH x 800 kHz) = 0.53 A pp at 26.4 V in. Eq 11 minimum is 0.25 x 3.3 / 800 kHz = 1.03 uH. At 1 A out the peak is about 1.27 A against Isat 5.7 A.
- Datasheet Table 9-1 has no 3.3 V row. Its 5 V rows use 2 to 3 x 22 uF; C5, C6 and C9 are 3 x 22 uF at 25 V X7R. DC-bias derating at 3.3 V is unknown and stability is unverified.

## Power-good

| Item | Value | Source |
| --- | --- | --- |
| Pin | U1 pin 6, open-drain NMOS | SNVSBM3B 8.3.5 |
| Pull-up | R_PG 100k to +3V3 | 8.3.5 allows a 100k pull-up to VOUT; the pin tolerates 20 V (7.1, 7.3) |
| Sink current | 33 uA through 100k, limit 5 mA DC | 8.3.5 |
| On-resistance | 92 ohm typ at VEN 3.3 V, 140 ohm at VEN 0 | 7.5 |
| Asserted low | during start-up, current limit and thermal shutdown, and when EN is low | 8.3.5 |
| Thresholds | rising 110 / 112 / 114 %, falling 90 / 92 / 94 % of VFB, hysteresis 2.2 % | 7.5 |
| Start-up delay | about 4 ms from EN asserted | 8.3.5 |
| Glitch filter | 40 us typ | 7.5 |
| Presented on | J2 pin 3 (3.3 V logic) | this board |

The netlist proves the connection (A15 in `QUALIFICATION.md`). Nothing proves the thresholds, the delay or
the behaviour on a fault: Synth has no power-good rule (`synth capability list` marks
`power_sequencing_and_power_good` unsupported), and this board was not powered.

## Thermal

- U1's exposed pad (pin 9, 2.41 x 3.1 mm, centre (33.0, 16.0) mm) is soldered to the F.Cu GND pour and holds
  a 2 x 3 array of six GND vias (0.6 mm pad, 0.3 mm drill, 1.2 mm apart), declared in
  `buck.placement.layout.toml` as `[[vias]]`. The PCB export writes them before routing, so the router
  treats them as GND copper. TI asks for at least a 4 x 3 array of 10 mil vias (9.5.1.1); six vias is fewer.
- Via-in-pad needs resin-filled and capped vias, so the board declares `manufacturer "jlc-filled-via"` and
  `requires via_in_pad`; `release.json` lists the six vias as fabrication notes.
- Counts from `kicad/buck.kicad_pcb` (`vias.py`, GND vias, distance from the pad centre): inside the pad 6;
  within 1 mm 2, within 2 mm 6, within 3 mm 6, within 4 mm 7. The board has 21 vias in total, all GND, all
  0.3 mm drill and 0.6 mm pad. The other 15 were placed by the router.
- Not estimated: TI's `RthetaJA` of 42.9 C/W is not valid for design (datasheet text near Eq 15), the LMR38010
  datasheet has no copper-area curve, and no efficiency curve was read. At 1 A out and an assumed 85% efficiency the loss
  would be 3.3/0.85 - 3.3 = 0.58 W; the junction temperature is left to the expert (B2).

## Parts and datasheets (read 2026-10-24 and 2026-10-29)

| Part | Datasheet | Revision | Values taken | Conflict or gap |
| --- | --- | --- | --- | --- |
| U1 TI LMR38010SDDAR (HSOIC-8 DDA) | https://www.ti.com/lit/ds/symlink/lmr38010.pdf | SNVSBM3B | 4.2-80 V recommended, 85 V abs max (SW 85 V); 1 A; 200-2200 kHz adjustable; internal compensation; VREF 1.0 V (0.985-1.015); EN rising 1.1/1.25/1.4 V, falling 0.95/1.10/1.22 V; PG tolerates 20 V; pins 1 GND, 2 EN, 3 VIN, 4 RT/SYNC, 5 FB, 6 PG, 7 BOOT, 8 SW, 9 EP; Cin at least 4.7 uF plus 100-220 nF (9.2.2.6) | Table 8-1 has no 800 kHz row; the RT table under Figure 8-4 gives 33k. Table 9-1 has no 3.3 V row |
| L1 Bourns SRN6045TA-6R8M | https://www.bourns.com/docs/Product-Datasheets/SRN6045TA.pdf | REV. 06/24 (footer) | 6.8 uH +-20%, DCR 34 mOhm typ, Irms 3.6 A, Isat 5.7 A (L -30%), 6.0 x 6.0 mm | Registry has no Isat field; Isat is in the description |
| D1 Vishay SS36 | https://www.vishay.com/docs/88751/ss32.pdf | 23-Apr-2020 (doc 88751) | VRRM 60 V, 3 A, IFSM 100 A, VF 0.75 V max at 3 A, IR 0.5 mA at 25 C and 10 mA at 100 C, SMC | Drawn with KiCad `Device:D_Schottky`; the footprint is `Diode_SMD:D_SMC`. VF at low current not read |
| D2 Vishay SMBJ30A | https://www.vishay.com/docs/88392/smbj.pdf | 09-Jan-2024 (doc 88392) | 600 W 10/1000 us, VWM 30 V, VBR 33.3-36.8 V at 1 mA, VC 48.4 V at 12.4 A, VBR temperature coefficient 0.099 %/C, SMB | No KiCad SMBJ30A symbol; Device:D_Zener used. Registry `max_voltage_v` 30 is VWM. The clamp is below U1's 85 V absolute maximum |
| F1 Bourns MF-MSMF110/33X | https://www.bourns.com/docs/Product-Datasheets/mfmsmf.pdf | REV. BE, 09/26 (footer) | Vmax 33 V, Imax 20 A, Ihold 1.1 A at 23 C, 0.93 A at 40 C, 0.73 A at 60 C, 0.63 A at 70 C, 0.50 A at 85 C, Itrip 2.2 A, R 0.06-0.20 Ohm | none found. Registry `max_current_ma` 1100 is Ihold at 23 C |
| J1, J2 JST B2B-XH-A / B4B-XH-A | https://www.jst-mfg.com/product/pdf/eng/eXH.pdf | none printed | 2.5 mm pitch, 3 A, 250 V | Existing registry `jst_xh_2pin`/`jst_xh_4pin` say 5 V / 100 mA; that conflicts with this sheet, so new parts were added |
| D3, D4 Lite-On LTST-C190KGKT | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF | Spec No. DS22-2000-074, Revision O, effective 09/04/2021 (read 2026-10-29) | see the review record below | none against the registry file; one footprint difference, below |
| C1 47 uF | Honor Elec RVT1H470M0810 (JLCPCB C2895739) | [LCSC datasheet](https://wmsc.lcsc.com/wmsc/upload/file/pdf/v2/lcsc/2109290630_Honor-Elec-RVT1H470M0810_C2895739.pdf) | 47 uF, 50 V, +/-20 %, 105 C, 8 x 10.2 mm SMD; ripple 120 mA at 120 Hz | ESR not given |
| C2, C5, C6, C9 (1206), C3, C4, C7, R_RT and the other resistors (0603) | none | | generic registry parts; voltage and dielectric are set per instance in `buck.synth` | no MPN. Murata and TDK pages returned 404/403/500: no DC-bias data |

### Review record: D3, D4 Lite-On LTST-C190KGKT

Read from the datasheet above on 2026-10-29. Registry file `registry/parts/diodes/ltst_c190kgkt.synth.toml`.

- Package: 1.6 x 0.8 mm body, 0.8 mm tall, 0.4 mm terminals, cathode marked on the package. Recommended pad 0.8 x 0.8 mm each, 2.3 mm outer span.
- Ratings: DC forward current 30 mA, power 75 mW, reverse voltage 5 V. Electrical: VF 1.9-2.4 V at 20 mA, IV 18-71 mcd at 20 mA, dominant wavelength 567.5-576.5 nm (green AlInGaP), IR 10 uA at 5 V.
- Registry: pin 1 cathode, pin 2 anode; `kicad_symbol` Device:LED (K=1, A=2); footprint `LED_SMD:LED_0603_1608Metric` (pad 1 cathode, pad 2 anode); `max_current_ma` 30 agrees with the DC rating; Vf 1.9 to 2.4 V in the description agrees; width 2.96 mm is the footprint courtyard width, not a datasheet figure.
- Footprint: the KiCad pads are 0.875 x 0.95 mm at +-0.7875 mm, an outer span of 2.45 mm. Datasheet: 0.8 x 0.8 mm and 2.3 mm. So the KiCad pads are 0.075 mm wider, 0.15 mm longer in Y, and the outer span is 0.15 mm larger. Polarity agrees. Pad shape differences are for the reviewer to accept.
- Circuit: D3 from +3V3 through R_LED 470 ohm: (3.3 - 2.4 to 1.9) / 470 = 1.9 to 3.0 mA, well under 30 mA. D4 likewise through R_PGLED.
- Not checked: availability, LCSC number, the Lite-On temperature derating curve against the board temperature.

Not signed. `reviewed_by` stays empty until the maintainer has read the datasheet and signs it in the registry file.

### Review record: U1 TI LMR38010SDDAR

Read from SNVSBM3B (text copy), 2026-10-29. Registry file `registry/parts/regulators/lmr38010.synth.toml`.

- Pins: registry gnd 1, en 2, vin 3, rt_sync 4, fb 5, pg 6, boot 7, sw 8, pad 9; the datasheet pin table agrees.
- Ratings used: operating 4.2 to 80 V (registry min 4.2, max 80, 1000 mA); absolute maximum VIN and SW 85 V, EN VIN + 0.3 V, FB and RT/SYNC 5.5 V, PG 20 V; VREF 1.0 V; junction temperature -40 to 150 C.
- Footprint: `HSOP-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.1mm`, exposed pad 2.41 x 3.1 mm. The datasheet DDA land pattern shows a 2.40 x 3.10 mm pad. The 0.01 mm difference is below the drawing tolerance. The signal pads are 1.6 x 0.6 mm on a 1.27 mm pitch; they were not compared with the DDA drawing line by line.
- Application: Cin at least 4.7 uF plus 100-220 nF (C2 10 uF, C3 220 nF); inductor above the Eq 11 minimum (1.03 uH against 6.8 uH); PG pull-up and EN divider as in the sections above.
- Not checked: availability, LCSC number.

Not signed. `reviewed_by` stays empty until the maintainer has read the datasheet and signs it in the registry file.

## Limits and honesty notes

- Overvoltage: the converter is rated 80 V operating and 85 V absolute maximum. SMBJ30A has VBR 33.3 to 36.8 V
  and VC 48.4 V at 12.4 A (Vishay smbj.pdf, 600 W 10/1000 us), below the 85 V maximum, so a surge the TVS can
  absorb leaves the regulator inside its absolute maximum. A SUSTAINED overvoltage is limited by other parts:
  the PTC F1 (Vmax 33 V) and the 50 V ratings of C1, C2 and C3. No surge test was run and no IEC 61000-4-5
  rating is claimed. The TVS energy, the PTC under surge and the capacitors at the 48.4 V clamp (50 V parts,
  1.6 V margin) are for the expert (B1). The TVS standoff (30 V) is above the 26.4 V maximum input, so it
  does not conduct on a healthy rail.
- PTC and rating: output current is rated 1 A (the converter's limit). At 1 A out the input current is
  3.3 / 0.85 / 24 = 0.162 A at 24 V and 0.240 A at the UVLO stop voltage (16.2 V), against F1 Ihold 0.63 A at
  70 C (3.9x and 2.6x). The 85% efficiency is an assumption.
- Output capacitors: C5, C6 and C9 are 22 uF 25 V X7R 1206. DC-bias derating at 3.3 V is unknown. The internal
  compensation is specified for a range of inductance and capacitance; stability is unverified.
- Input capacitors: the datasheet asks for at least 4.7 uF plus 100-220 nF. C2 is 10 uF X7R 50 V; its effective
  value at 24 V is unknown. C1 47 uF electrolytic adds bulk with unknown ESR.
- Reverse polarity: Schottky chosen over a P-FET. SS36 VRRM 60 V against 26 V. Drop 0.75 V max at 3 A,
  so under 0.13 W at 0.162 A (bound; VF at low current not read). A P-FET at 24 V needs a gate clamp;
  no P-FET datasheet was read.

## Commands

```
export XDG_DATA_HOME=$(mktemp -d)   # empty dir: a user overlay in ~/.local/share/synth shadows registry parts
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
scripts/setup-routing-engines.sh --freerouting-only          # FreeRouting jar + Java, or symlink tools/freerouting and tools/jre25
B=qualification/class-b/buck                                 # relative to the repo root
synth validate $B/buck.synth
SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad $B/buck.synth --out out/buck \
  --allow-unverified-parts --validate-erc --router free-routing --pnp --gerbers --drill   # fab package: reports/release-fab.json
SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad $B/buck.synth --out out/buck-nofab \
  --allow-unverified-parts --validate-erc --router free-routing                           # source/DRC gate: reports/release.json
kicad-cli pcb drc --severity-all --refill-zones --format json -o $B/reports/buck.kicad-drc.json $B/kicad/buck.kicad_pcb
kicad-cli sch erc --severity-all --format json -o $B/reports/buck.kicad-erc.json $B/kicad/buck.kicad_sch
synth validate $B/buck_neg_fb_sw.synth
python3 -I $B/pad_gaps.py $B/kicad/buck.kicad_pcb
```

`out/` is gitignored scratch. The sidecar `buck.placement.layout.toml` decides placement; there is no
`--place-route-iterate`. The two `export-kicad` runs differ only in whether the fab gate is asked, which is why
there are two release reports. The repository root prefix was stripped from the JSON reports with `sed`.
Registry parts were authored or imported with `synth part import-kicad` and then edited by hand (names, MPN,
dimensions, datasheet URL, `switch_node` on SW and `feedback` on FB); `lmr38010` is hand-written from the
datasheet.

## Measured results

The export routes with FreeRouting 2.4.1 on OpenJDK 25.0.4.1; the routed board is written as `out/buck/buck.kicad_pcb` when Synth's gate passes (state `routed`). Numbers below are from the committed files, measured 2026-10-29. `reports/buck.routing.json` records `source_revision` a9c7546e4aeecee7719aa003f3699064bb30b912, the commit of the sources the board was built from.

| Number | Value | Command |
| --- | --- | --- |
| validate, good board | exit 0, 0 errors, 14 warnings (3 W-SYNTH-PART-UNVERIFIED, 3 E-SYNTH-SCHEM-003, 1 E-SYNTH-SCHEM-004, 4 E-SYNTH-SCHEM-005, 1 W-SYNTH-ANOMALY-001, 2 W-SYNTH-SUPPLY-002) | `synth validate $B/buck.synth 2>&1 \| grep -o '^warning: \[[A-Z0-9-]*\]' \| sort \| uniq -c` |
| validate, negative | exit 1, 1 error (E-SYNTH-POWER-011), the same 14 warnings | same on `buck_neg_fb_sw.synth` |
| routing | `routed`, 13 nets connected, 0 open, 97 segments (all F.Cu), 21 vias, 200.08 mm | `reports/buck.routing.json` `statistics`; the PCB file also holds 97 segments and 21 vias |
| Synth routing check | kicad_drc 0 errors, 0 unconnected, 0 warnings | `reports/buck.routing.json` `validation` |
| Release gate | `release_ready` true, `production_status` production, 0 signal-integrity findings (0 `W-SYNTH-DRC-016`); with `--gerbers --drill` the same board reports `release_ready` false and `untrusted` because 2 parts have no reviewer (`lmr38010`, `ltst_c190kgkt`: "2 part(s) have no reviewer") | `reports/release.json`, `reports/release-fab.json` |
| DRC | 0 errors, 0 unconnected, 0 warnings | `kicad-cli pcb drc ... -o reports/buck.kicad-drc.json` |
| ERC | 0 violations | `kicad-cli sch erc ... -o reports/buck.kicad-erc.json` |
| Schematic netlist | 13 nets: the source's 7 `net` blocks plus the 6 further names its 10 `connect` statements create (`+3V3` joins an existing net), node for node | `kicad-cli sch export netlist --format kicadsexpr -o /tmp/buck.net $B/kicad/buck.kicad_sch` |
| Outline | 70.99 x 33.99 mm (x 0.55 to 71.54, y 0.55 to 34.54), 2413 mm2 | `grep -A3 gr_line $B/kicad/buck.kicad_pcb \| grep -E 'start\|end'` |
| Manufacturing exports | 22 Gerbers plus the job file (4 copper layers), PTH 27 holes (21 x 0.3 mm vias, 4 x 0.95 mm and 2 x 1.0 mm connector pins), NPTH 2 x 2.5 mm mounting holes | `ls $B/kicad/gerbers $B/kicad/drill`; `grep -c '^X' $B/kicad/drill/buck-PTH.drl` |
| Hashes | full values in `SHA256SUMS` and `QUALIFICATION.md` | `cd $B && grep -v '^#' SHA256SUMS \| sha256sum -c -` |

Pad-edge gaps in the placed, routed board, from `pad_gaps.py` (pcbnew pad bounding boxes, edge to edge):

| Pair | Gap mm |
| --- | --- |
| C3 (220 nF) to U1 VIN | 0.82 |
| C2 (10 uF) to U1 VIN | 1.46 |
| C3 / C2 ground pad to U1 GND | 2.69 / 5.35 |
| C4 to U1 BOOT / SW | 0.62 / 2.18 |
| R_RT to U1 RT/SYNC | 1.22 |
| L1 pad 1 to U1 SW | 3.80 |
| C5, C6, C9 to L1 pad 2 | 0.83 / 2.29 / 0.70 |
| D2 TVS to U1 VIN | 7.82 |
| R_UV_T to U1 EN | 3.35 |
| R_PG to U1 PG | 22.63 |
| R_FB_T / R_FB_B to U1 FB | 3.55 / 3.79 |
| C7 to R_FB_T (FB side) | 0.55 |
| R_FB_B ground pad to U1 GND | 11.31 |
| R_FB_T FB pad to L1 pad 1 (SW) | 2.58 |

## Placement

The placement is a hand-pinned layout in `buck.placement.layout.toml`: all 27 parts are pinned with hard
priority, plus two label offsets for R_FB_T and nothing else. Synth's unassisted auto placement was dropped
because it left C3 19.8 mm from VIN, the output capacitors 12.9 to 19.3 mm from L1 and 8 W-SYNTH-DRC-016
return-path warnings (a reviewer's finding). Choices:
- J1 -> F1 -> D1 sit below U1, so VIN does not cross the EN divider.
- C3 and C2 sit left of U1 at the VIN pad; C4 beside BOOT; R_RT beside RT/SYNC.
- C5, C6 and C9 form a short column right of L1.
- J2 is rotated 270 degrees so PG (pin 3) and +3V3 (pin 1) do not cross; R_PG and the PG LED chain sit beside J2.
- R_PG is 22.63 mm from the PG pin: the pull-up serves J2, and the PG track is a signal.

`pad_gaps.py` on the final routed file is the reference for gaps. The outline follows the pinned part
positions and the two corner holes; it is larger than the earlier auto placement of the LMR33640 version
(60.75 x 31.855 mm), and I did not isolate the cause of the 478 mm2 growth.

## Negative case: feedback from the switch node

`synth validate $B/buck_neg_fb_sw.synth` exits 1 (JSON, schema 1.3):
```
error: [E-SYNTH-POWER-011] regulator feedback is taken from the switching node
  found:    feedback pin `U1.fb` (group "FEEDBACK") is joined through resistor `R_FB_T` to switch-node pin `U1.sw` (group "BUCK")
  expected: the feedback pin to sense the regulated output, not the switching node
```
`synth export-kicad` stops with "Synth ERC validation failed; use --force to export anyway" (exit 2). The good board does not raise it. The rule follows one resistor only.

## Qualification matrix

The matrix (A: checked by tools, B: needs an expert) is in [`QUALIFICATION.md`](QUALIFICATION.md),
together with the `#38` checklist traceability for this board. Its row for the corpus is in
`qualification/matrix.toml` (section D there). Evidence files: `kicad/`, `reports/`, `images/`.

## Not true yet

- Not fabricable on this evidence: two parts have no reviewer, nothing was built, no surge test was run. The
  fab package says so itself (`release_ready: false`, `production_status: untrusted`).
- Layout: current loop area, return-current paths and EMI were not analysed. The layer changes are the 21 GND
  vias; no signal net changes layer (all 97 segments are on F.Cu).
- Six thermal vias, where TI asks for at least 4 x 3 (9.5.1.1).
- Silkscreen: the "PG FAULT" and "PWR" LED values are on the F.Fab layer only, so the board prints just the references D4 and D3. Synth has no silk text for a value here. KiCad DRC reports no silkscreen violation.
- Schematic (rendered by `tools/schematic_quality.py`): 0 overlapping text pairs, 7 texts with a wire through them, 1 wire crossing, 1313.2 mm of wire; mean passive-to-pin distance 66.4 mm (max 172.8). No schematic sidecar is used.
- Routing used FreeRouting defaults, no length or coupling constraints.

## Not verified

- Nothing powered, measured or thermally tested. Efficiency (85%) and the 0.58 W loss are assumptions.
- The MCP placement review (`synth_describe_placement`) was not rerun on the hand-pinned placement.
- No reviewer on `lmr38010` and `ltst_c190kgkt`. Availability, LCSC numbers and assembly not checked. Generic passives have no MPN or manufacturer sheet.
- PTC behaviour with the TVS in a surge, and inrush into C1 and the output caps, are not analysed.
- The Gerbers and drill files were not opened in a fab's DFM tool and no fab accepted them; the via-in-pad process was not confirmed with a fab.
- `synth check` on this board was not run to a verdict: it selects KiCadRoutingTools, which was not installed (the DRC stage reports `unknown`).
- Hash reproducibility on another machine depends on FreeRouting 2.4.1, the Java build and KiCad 10.0.7.
