# ice40_bga_node: USB-C input and STM32F103 companion MCU (revision report)

Outcome: **not released.** Export 3 routed all nets with 0 KiCad DRC errors, but `state` is `validation_failed` and `fabrication_ready` is false (one return-via blocker). The outline is about 113.5 x 82.8 mm, not 44 x 40 mm. Fill was not measured (see "What I could not do").

## Design decisions (all in `ice40_bga_node.synth`)
- J1 is now `usb_c_receptacle` (`escape { off }`, hint `edge: left`). VBUS pins (A4, A9, B4, B9) go to net VBUS, then the existing F1 polyfuse, D1 TVS, Q1 P-FET (order kept). All GND pins and the shield (`shld`) go to GND. SBU1 and SBU2 are left unconnected.
- R_CC1 and R_CC2: 5.1 kohm from CC1 and CC2 to GND, hinted near J1.
- U5 `stm32f103c8_usb` (LQFP-48, `escape { off }`), group "MCU". Supply pins sit on net `+3V3_MCU`: one 100 nF per pin (VDD, VDD_2, VDD_3, VDDA, VBAT), plus 4.7 uF, each hinted near its pin. Reset: 10k pull-up R21, 100 nF C33, button SW1. BOOT0: 10k pull-down R22. SWD header J4 (1 = 3V3, 2 = SWDIO/PA13, 3 = SWCLK/PA14, 4 = NRST, 5 = GND). USB: PA12 (D+) and PA11 (D-) through 22 ohm R25 and R26 to J1 DP/DN (both A and B pin pairs tied).
- Programming path: PA4 to CFG_SS_B, PA5 to CFG_SCK, PA7 (MOSI) to CFG_SDO (flash IO0 = DI), PA6 (MISO) from CFG_SDI (flash IO1 = DO), PB0 to CRESET_B, PB1 to CDONE (joined onto the existing nets). The bitstream is 135,183 bytes, larger than the MCU's 64 KB flash, so the SPI flash U2 stays the bitstream store and the MCU only streams it from the host. Firmware must tri-state PA4 to PA7 and release CRESET_B (open drain or input) when it is not programming, because the FPGA is the SPI master in boot mode.
- FB2 (ferrite bead, `ferrite_bead_0603`) isolates `+3V3_MCU` from `+3V3`. I added it because the plane fanout could not stitch +3V3 into the LQFP pads (defect 1). It is also a normal MCU supply filter. It is a design addition that was not in the brief.

## Crystal arithmetic
Y1 = NX3225GD-8MHZ, CL = 8 pF (registry description). Stray capacitance assumed 3 pF (pad, track and pin; an assumption, not measured).
C1 = C2 = 2 x (CL - Cstray) = 2 x (8 - 3) = 10 pF. C34 and C35 are 10 pF, 0402.

## Net classes
| Class | Edge rate | Why |
|---|---|---|
| PWR_5V, PWR_3V3, PWR_CORE, PWR_PLL, PWR_MCU, GND | slow | DC rails |
| STATIC, CONFIG, FPGA_IO | slow | static or low-rate signals (config SPI is a few MHz) |
| MCU_IO | slow | reset, BOOT0, SWD, crystal pins (declared slow; see defect 3) |
| USB | fast | D+/D- full speed; declared `layers "F.Cu", "B.Cu"`, width 0.12 mm, clearance 0.127 mm |

D+/D- pairs: `diff_pair USB_DP USB_DM` and `diff_pair USB_DP_C USB_DM_C`, 90 ohm target. Synth's estimate for the width and gap above is not 90 ohm (see defect 4).

## Exports (3 of 6 used; export_1 failed before routing)
| Export | State | DRC errors | Open nets | Blocking reasons |
|---|---|---|---|---|
| export_1 | export error (no board) | n/a | n/a | "power-plane fanout failed: net +3V3: the pad at (18.44, 38.85) mm on Top is 1.48 x 0.30 mm and a stitching via needs a 0.35 mm pad" |
| export_2 | validation_failed | 3 | 0 | 3 clearance errors with the USB class clearance 0.2 mm (CC1 pad A5 to USB_DM_C track 0.16 mm, USB_DP track to CFG_SDO via 0.13 mm, USB_DP_C to USB_CC2 0.18 mm); return via missing at (9.40, 30.00) mm |
| export_3 | validation_failed | 0 (74 warnings) | 0 | return via missing at (9.40, 30.00) mm (the via sits in pad Y1.1) |

export_3 detail: 42 nets connected, 398 segments, 212 vias, 1520.7 mm of wire, `engine_policy_verdict: rejected`, `fabrication_ready` false. The 41 other layer changes are advisories (slow nets).

Outline: the placer reports 113.54 x 82.85 mm for this source (no sidecar was used). The previous board was 44 x 40 mm, so **the outline grew and the "must not grow" requirement is not met.**

## Synth defects and limitations hit
1. **Plane fanout vs fine-pitch pads.** With the MCU supply on the plane net +3V3, export aborts: a 0.35 mm stitching via cannot fit in a 0.30 mm LQFP pad, whatever the footprint. The message is accurate. A plane net with any fine-pitch (0.5 mm pitch) pad cannot be exported. I worked around it by moving the MCU pins onto a non-plane net behind a ferrite bead.
2. **No placement sidecar in the workspace.** `ice40_bga_node.placement.layout.toml` does not exist, so the "previous 44 x 40 mm pose" cannot be reproduced. The auto-placer ignores the free area to the left of U1 and gives 113.5 x 82.8 mm, with the original parts also spread out. `synth_compact_layout` needs a sidecar, so I could not densify.
3. **Return-via blocker on a net declared slow.** The via at (9.40, 30.00) mm (Y1.1, the crystal pad, in the HSE pair `MCU_IO` class declared `edge_rate slow`) is reported as blocking, while the same finding on other slow nets is only an advisory. The report does not say which net it is, and I could not tell whether the crystal nets are auto-classified as Clock and override the declared class. I could not clear it from the source.
4. **Impedance check:** with width 0.2 mm and gap 0.127 mm Synth estimates 69.4 ohm (error E-SYNTH-IMPEDANCE-001, blocks validate); width 0.12 mm with gap 0.2 mm gives 101.9 ohm (warning). I use 0.12 mm / 0.127 mm in export 3; I did not re-run validate on that exact geometry to read its estimate. The USB class clearance is also applied to CC pads and other nets, which caused the 3 DRC errors in export_2.
5. The auto-placer put the crystal about 12 mm from the MCU (functional warnings: load caps 26 to 29 mm from the crystal), even with `near` hints. Hints are not honoured to the stated limits when the board has no sidecar.
6. **Registry gaps** (warnings, not fixed): header_1x5_swd, ferrite_bead_0603 and the test points have no MPN or LCSC number; `stm32f103c8_usb` has no reviewer.

## What I could not do
- Release a routed, DRC-clean, fabrication-ready board (one blocker left).
- Keep the outline at 44 x 40 mm, or measure courtyard fill. I had no fill tool; the placer-reported size is 113.54 x 82.85 mm.
- Check the plane/ground return arguments on the layer-change vias beyond Synth's own report.
- Verify the footprints/pin numbers of the USB-C receptacle (HRO TYPE-C-31-M-12) against its datasheet, or the ESD warning on D+/D-/CC (E-SYNTH-ESD-001; no ESD diode was added, none was requested).
- Run ERC, three identical exports, or a STEP export. Not attempted.
