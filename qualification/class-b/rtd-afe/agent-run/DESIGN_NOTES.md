Design notes reconstructed from the committed files. The board was built by agents from the briefs in `prompt.md`, copied verbatim from the session transcripts. No tool-call log or report was saved.

# DESIGN_NOTES: PT100 4-wire RTD analog front end (rtd-afe, SYN-363)

Sources: `rtd_afe.synth`, `README.md`, `QUALIFICATION.md`, `kicad/bom.csv`, `reports/*.json`, `registry/parts/**`. Anything not recorded there is marked "not verified".

## Purpose
Bench or lab 4-wire PT100 front end with a USB readout. README: "a compact mixed-signal board on a single ground plane, not an instrument"; not for a product or safety function until the QUALIFICATION.md review items are closed. Status: design, placement and a FreeRouting route that passes Synth's routing and DRC gates; not fabricated, not tested on hardware.

## Architecture
| Block | Parts | Role (from `.synth` notes) |
| --- | --- | --- |
| RTD input | J2, R_S1, R_S2, C_D, C_CM1, C_CM2, C_F, R_BIAS | J2 pins F+, S+, S-, F-; RC filter to ADC AIN1/AIN2 |
| ADC | U1 ADS124S08, C_AV1, C_AV2, C_DV, C_IO, C_RO, R_RST | 24-bit delta-sigma, PGA, IDAC1 excitation |
| Reference | U3 REF5025, C_RI, C_RV | 2.5 V absolute reference on REFP0/REFN0 |
| Analog supply | L2 (600R@100MHz ferrite bead) | +3V3 to +3V3A for U1 AVDD and U3 VIN |
| MCU | U2 RP2350, U4 W25Q128 flash, Y1 12 MHz, L1, J3 SWD and support passives | SPI master to U1, QSPI boot flash, on-chip 1.1 V regulator |
| USB-C readout | J1, R_CC1/2, R_DP/DN, D1/D2, U5 LP5907-3.3, C_L1, C_L2 | USB 2.0 device, VBUS powers the +3V3 digital rail |
| Mechanical | H1..H4 | M2.5 non-plated mounting holes |

## Signal chain and excitation
- IDAC1 sources 1 mA out of AIN0 into F+, through the PT100 and F- into R_BIAS (1k) to GND. R_BIAS sets RTD common mode near 1.1 V (README: about 1.0 V plus the RTD drop; 1.1 V at 100 ohm, 1.25 V at 250 ohm).
- REF5025 is the absolute reference (REFP0/REFN0), not ratiometric to the IDAC. README lists this as a deliberate trade and an open accuracy item.
- Input filter: R_S1/R_S2 1k 0.1 %, C_D 100 nF C0G across the pair, C_CM1/C_CM2 10 nF C0G to GND, C_F 10 nF C0G on F+. The `.synth` note gives about 720 Hz differential and about 16 kHz common mode; README recomputes about 758 Hz (105 nF equivalent) and 15.9 kHz. The difference is the assumed equivalent capacitance (110 nF vs 105 nF).
- C_D is 10x each C_CM so capacitor mismatch gives little differential error; all filter caps are C0G.
- The 1 mA IDAC setting, gain and data rate are firmware settings. No firmware exists in the repository: not verified.

## Decisions recorded in the sources
1. One GND plane on all four layers; no AGND/DGND split, no tie. The split version left two DGND pour islands the router could not tie (QUALIFICATION.md, DRC history).
2. ADC returns its own digital supply current to the same plane: AVSS, AVSS_SW, REFN0, REFCOM, EP, DGND (pin 14) and CLK (pin 17, internal oscillator) are on GND.
3. AVDD and U3 VIN from +3V3A through ferrite L2; DVDD and IOVDD from +3V3.
4. SPI0 wired per RP2350 silicon: GPIO0 MISO, GPIO1 CSn, GPIO2 SCK, GPIO3 MOSI. The registry lists gp0 as spi_mosi and gp3 as spi_miso, the opposite; the wiring follows the silicon (`.synth` comment). Datasheet check of this: not verified in this reconstruction.
5. RP2350 uses its on-chip regulator: VREG_LX through L1 3.3 uH to DVDD_1V1; VREG_FB on that net.
6. VREG_AVDD and ADC_AVDD from +3V3 through R_AV 33R and C_AVF 4.7 uF.
7. Crystal: ECS-120-20-33-TR, CL 20 pF, 2x 33 pF C0G; `.synth` note: 33/2 + about 3 pF stray = 19.5 pF; R_X 1k on XOUT. The 3 pF stray is an assumption in the note: not verified.
8. Via-in-pad: `requires via_in_pad`, manufacturer `jlc-filled-via`; one via-in-pad at C_DV.2 (16.5, 22.3) mm; board must be ordered with filled and capped vias.
9. Stackup (declared, 4-layer, 1.5862 mm): 0.035 Cu / 0.2104 prepreg er 4.4 / 0.0152 Cu / 1.065 core er 4.6 / 0.0152 Cu / 0.2104 prepreg er 4.4 / 0.035 Cu. No controlled impedance requested. USB netclass: trace 0.242 mm, clearance 0.127 mm.
10. Board: 40.1 x 44.2 mm rectangle, 1772 mm², 53 footprints, 40 nets; J1 centred on the top edge; J2/J3 on the bottom edge.

## Known gaps (copied from README/QUALIFICATION.md)
- No VBUS fuse, TVS or reverse-polarity protection; CC1/CC2 no TVS; shield tied directly to GND.
- D1/D2 are the generic `esd_usb` part with no MPN or datasheet.
- No test points (a 12-pad set left GND islands and was dropped).
- Unused ADC inputs AIN3..AIN11, AINCOM, REFP1/REFN1, GPIOs and pin 25 are left open; termination not datasheet-reviewed.
- No firmware; no VBUS current budget measured; no calibration or IDAC accuracy check.
- No board name/revision/pin-function silk (Synth has no free-text silk).
- 34 reference labels hidden on silk (list in QUALIFICATION.md).
- Y1 has no part-level reviewer; this is the release override.
- Mounting holes leave 2.25 to 2.34 mm material; J2/J3 0.71 mm from the bottom edge, J1 1.27 mm from the top. Enclosure fit not checked.
