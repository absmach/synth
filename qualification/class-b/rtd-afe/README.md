# PT100 4-wire RTD analog front end (Class B, SYN-363)

A 4-wire PT100 front end with a USB readout: a TI ADS124S08 24-bit delta-sigma ADC with IDAC excitation, a TI REF5025 2.5 V reference and a Raspberry Pi RP2350A host on USB-C. The board is 40.0 x 44.2 mm on four copper layers (1.6 mm FR4), with one GND plane.

It answers SYN-363: a compact mixed-signal board where a precision analog chain shares a board and a ground plane with a USB microcontroller. It is meant for bench or lab use, not as an instrument, a product or a safety function.

The board is untrusted and not release-ready. The committed KiCad, fab, image and STEP files are historical output from source revision `01a77d54`. The current tree does not route this board, so they are not reproducible from head. The board is not fabricated and not tested on hardware. No analog accuracy is claimed.

Authored by nyagamunene. The SI/PI/RF review by the designated reviewer and the issue owner's sign-off on the single ground plane are open.

The prompt used to produce the board is in [`PROMPT.md`](PROMPT.md).

## The board

| Group | Parts |
|---|---|
| RTD input | `J2` 1x4 2.54 mm header (pin 1 F+, pin 2 S+, pin 3 S-, pin 4 F-); `R_S1`, `R_S2` 1k 0.1 % series resistors on the sense pair; `C_D` 100 nF X7R across the pair; `C_CM1`, `C_CM2` 10 nF C0G to GND; `C_F` 10 nF C0G on F+; `R_BIAS` 1k 0.1 % from F- to GND |
| ADC | `U1` TI ADS124S08IRHBR (VQFN-32, exposed pad), AIN0 force output, AIN1/AIN2 differential sense input; `C_AV1`, `C_AV2`, `C_DV`, `C_IO` decoupling; `C_RO` 1 uF on REFOUT; `R_RST` 10k pull-up on RESET, also driven by the MCU |
| Reference | `U3` TI REF5025AIDR 2.5 V on REFP0/REFN0, with `C_RI` and `C_RV` 1 uF |
| Analog supply | `L2` Murata BLM18AG601SN1D ferrite bead (600 ohm at 100 MHz) makes `+3V3A` for `U1` and `U3` |
| MCU | `U2` RP2350A (QFN-60) on its on-chip regulator, `L1` 3.3 uH (Sunlord MPL2012S3R3MHT, 0805) to the 1.1 V DVDD net; `U4` W25Q128JVSIQ QSPI flash (SOIC-8 208 mil); `Y1` ECS-120-20-33-TR 12 MHz crystal with 33 pF C0G `C_X1`, `C_X2` and `R_X` 1k on XOUT; `R_AV` 33 ohm and `C_AVF` for VREG_AVDD and ADC_AVDD; `R_RUN` 10k; `J3` 1x3 SWD header (SWCLK, SWDIO, GND) |
| USB-C readout | `J1` HRO TYPE-C-31-M-12; `R_CC1`, `R_CC2` 5.1k; `R_DP`, `R_DN` 27 ohm; `D1`, `D2` ESD5V0B03-323 on D+/D-; `U5` TI LP5907MFX-3.3 LDO (250 mA) makes `+3V3` from VBUS, with `C_L1`, `C_L2` 2.2 uF |
| Mechanical | `H1` to `H4` M2.5 non-plated holes (2.7 mm) at (6, 3.5), (39, 3.5), (6, 40.7) and (39, 40.7) mm |

Signal chain: J2, then the RC filter, then U1 AIN1/AIN2. IDAC1 sources 1 mA (firmware setting) out of AIN0 into F+, through the PT100 and F-, and into `R_BIAS` to GND. `R_BIAS` lifts the RTD common mode to about 1.1 V at 100 ohm and 1.25 V at 250 ohm.

The reference is absolute, not ratiometric. VREF is 2.5 V from U3. The IDAC's absolute accuracy and drift therefore limit the measurement unless the firmware calibrates them. This trade is deliberate.

Nominal filter corners, assuming the 2 k filter resistance dominates the 100 to 250 ohm PT100:

| Path | Elements | Corner |
|---|---|---|
| Differential | R_S1 + R_S2 = 2 k; C_D 100 nF + C_CM1/C_CM2 in series (5 nF) = 105 nF | about 758 Hz |
| Common mode | R_S 1 k, C_CM 10 nF | about 15.9 kHz |

`C_D` is ten times each `C_CM`, so capacitor mismatch turns into little differential error. `C_D` is X7R because no 0603 C0G 100 nF is stocked. The `.synth` note says "about 720 Hz"; it assumes 110 nF instead of 105 nF.

Ground: one GND plane on all four copper layers, with no AGND/DGND split and no tie resistor. The ADC's AVSS, AVSS_SW, REFN0, REFCOM, exposed pad, DGND (pin 14) and CLK (pin 17, internal oscillator) return to the same plane as the MCU, flash, USB and LDO. Issue #363 asked for a split; this is a deviation.

Layout: analog on the left, digital on the right. The outline is a plain rectangle from the placement sidecar. The committed `kicad/rtd_afe.kicad_pcb` holds 389 track segments and 89 vias. The source declares `requires via_in_pad` and the `jlc-filled-via` manufacturer profile.

## Not verified by Synth

- **Route and release:** the current tree does not route this board. The DRC, ERC, signal-integrity and release results of the historical route are no longer in the tree and are not checked in this folder.
- **Via in pad:** the historical route was recorded with one via in a `C_DV` pad (order filled and capped vias). That record is no longer in the tree, and JLCPCB has not accepted the process.
- **Fab BOM:** `fab/bom.csv` predates the source pins for `C_D`, `L1` and `Y1`. It lists `C_D` as C0G with no part number, and `L1` and `Y1` without one. The source pins C14663 (X7R), C98331 and C2449814.
- **Design note:** a USB note in `rtd_afe.synth` points at a qualification file that is no longer in the tree.
- **VBUS protection:** no VBUS fuse, TVS or reverse-polarity stage. CC1/CC2 have no TVS. The USB shield is tied directly to GND.
- **3.3 V budget:** the LP5907 is 250 mA. The RP2350, ADC, reference and flash loads were not checked against it.
- **RP2350 decoupling:** four IOVDD caps for six IOVDD pins and two DVDD caps plus `C_DVB` for three DVDD pins, below the one-per-pin recommendation of Raspberry Pi's hardware design guide. `L1` DCR and saturation current are not checked.
- **Analog:** accuracy, noise, linearity, drift and the PT100 reading; IDAC accuracy; the PGA common-mode limit at the chosen gain (SBAS660C); the RC filter against the ADC data rate.
- **Unused pins:** AIN3 to AIN11, AINCOM, REFP1/REFN1, the ADC GPIOs and pin 25 are open, with no datasheet review of their termination.
- **Firmware:** gain, data rate, IDAC value, register map, USB descriptors and calibration do not exist in this repository.
- **No test points:** earlier attempts to add them left GND islands.
- **Silkscreen:** no board name or revision, and no header pin labels. Some reference labels are hidden.
- **Other:** crystal ESR and drive level, QSPI timing, USB signal integrity, ESD, surge, USB-C compliance, mechanical fit and fabrication. Nothing was sent to a fab house.
- **Review:** the SI/PI/RF review and the issue owner's sign-off on the single ground plane are open.
