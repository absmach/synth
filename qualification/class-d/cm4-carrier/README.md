# Raspberry Pi CM4 industrial carrier (SYN-407)

Status: source, parts, placement sidecar and tests are in the tree. The
board is **not fabricated, not powered and not hardware-tested**. The
schematic export takes about 30 minutes on this board and the PCB export did
not finish; `kicad/` therefore has the four-sheet schematic but no
`.kicad_pcb`, and `images/` has the schematic SVGs but no copper or 3D
renders. See `QUALIFICATION.md` and `agent-run/VERIFICATION_REPORT.md`.
`qualification.json` stays `"qualified": false`.

This is Class D board 1 of 4 under #41. It asks whether Synth can produce an
auditable carrier for a Linux compute module with many high pin-count
interfaces and a multi-rail power chain, while stating plainly what it
cannot verify.

## What

- `cm4_carrier.synth` — the only editable source.
- `cm4_carrier.placement.layout.toml` — board-millimetre placement sidecar.
- `crates/synth-cli/tests/cm4_carrier_board.rs` — the pin-map and mounting-hole
  regression tests.
- `qualification.json` — gate state; `qualified` is false.
- `BOM_sourcing.md` — per-part MPN, LCSC number, alternate, datasheet.
- `agent-run/` — prompt, tool calls, design notes, pin map, netlist, power
  budget, assembly, bring-up and verification report.

## Power tree

```
J1 12 V -> F1 2.6 A PTC -> Q1 DMP3098L reverse-polarity P-FET -> VIN_PROT
VIN_PROT -> D1 SMAJ15A TVS, C1 47 uF, C2 10 uF, C3 220 nF
VIN_PROT -> U1 LMR33640 4 A, 400 kHz -> L1 6.8 uH -> +5V
+5V -> U2 LMR38010 1 A, 800 kHz -> L2 6.8 uH -> +3V3
+5V -> U3 LMR38010 1 A, 800 kHz -> L3 6.8 uH -> +1V8
+5V -> L_FB1 ferrite bead -> +5V_AUX -> U2/U3 inputs
```

- **+5V** (4 A): CM4 +5V pins, four USB port switches, USB hub 3V3 LDO input
  path, HDMI 5 V switch, fan header.
- **+3V3** (1 A): USB2514B, microSD switch input, both FFC connectors, HDMI
  DDC A side, RTC, LEDs, 40-pin header 3V3 pins.
- **+1V8** (1 A): optional CM4 `GPIO_VREF` selection on `J_VREF` (default is
  +3V3); no other load.

### Sequencing (designed in, not verified by Synth)

`PG_5V` (open drain, 100k to +5V) drives `U2.EN`. `PG_3V3` (open drain, 100k
to +3V3) drives `U3.EN`. `PG_1V8` is a test point. The CM4 has its own
internal sequence (+5V -> GLOBAL_EN -> CM4_3.3V -> CM4_1.8V -> RUN_PG ->
nEXTRST); the carrier rails come up after +5V is good and are independent of
the module's internal rails. `GLOBAL_EN` is a tactile switch to GND with an
external 100k pull-up to +3V3 and 100 nF; `nRPIBOOT` is a 2-pin jumper to GND.

Synth has no sequencing check. The intended order is from the CM4 datasheet
section 5.1 (page 25): +5V, GLOBAL_EN, +3.3V, +1.8V at least 1 ms after
+3.3V, RUN_PG at least 10 ms after +1.8V, nEXTRST at least 1 s after RUN_PG.

## CM4 interface

`J_CM4A` is CM4 pins 1–100 and `J_CM4B` is pins 101–200; both are the Hirose
DF40C-100DS-0.4V(51) receptacle (LCSC C597931), the mating height of the CM4
module is 1.5 mm. The parts are authored with pin names `p1`…`p100`; the pad
number on each connector is the CM4 pin number on A, and 100 + the CM4 pin
number on B. 143 pins are used; 57 are NC and are listed in the `CM4 MODULE`
notes and in `agent-run/PIN_MAPPING.md`.

Highlights:

- Ethernet: 4 pairs on CM4 pins 3–12 to the UDE RB1-125B8G1A GbE magjack (TD1–TD4).
- USB 2.0: CM4 pins 105/103 to the USB2514B upstream port.
- HDMI0: 4 TMDS pairs, CEC, HPD, DDC on CM4 pins 151–200 (the second
  connector). HDMI1 is NC.
- microSD: CM4 Lite SD_CLK/CMD/DAT0–3, SD_PWR_ON; DAT4–7 are NC.
- CAM0 and DSI0: 2-lane 22-pin FFCs. CAM1 and DSI1 are NC.
- 40-pin GPIO: all 28 CM4 GPIOs, 3V3, 5V and GND in the Raspberry Pi pinout.
- RTC: PCF85063ATL on SCL0/SDA0, INT on GPIO26.
- `GPIO_VREF` is selected by `J_VREF`: 1–2 = +3V3 (default), 2–3 = +1V8.

## USB hub

USB2514B (LCSC C633338), self-powered, `CFG_SEL[1:0] = 00`, individual port
power switching and individual over-current sensing. 24 MHz crystal, 12 kΩ
RBIAS, 100 nF on CRFILT and PLLFILT, 10 kΩ straps. `PRTPWR1–4` drive four
TPS22918 load switches (one per port); `OCS_N1–4` are tied high because there
is no current-sense device. Each port has a PRTR5V0U2X ESD array. Two stacked
USB-A receptacles (Same Sky UJ2-ADH-3-TH) give four ports.

## Ethernet

UDE RB1-125B8G1A 10/100/1000BASE-T magjack, 1:1, through-hole. The four CM4
pairs go to TD1–TD4; the single centre tap (R1) is decoupled to GND with
100 nF, following the CM4IO board reference. The magjack's own LEDs are not
used; the CM4 Ethernet LED outputs drive the board LEDs (active low). Shield
and R10 to GND.

## HDMI

HDMI0 only. The four TMDS pairs go to an HDMI-A receptacle with a
PRTR5V0U2X on each pair. DDC (SDA/SCL) is shifted between 3.3 V and 5 V by a
TXS0108E; 2.2 kΩ pull-ups are fitted on both sides. CEC and HPD are direct.
The connector +5V is switched by a TPS22918 with 1 µF + 100 nF.

## microSD, FFC, GPIO, RTC, fan

- microSD: Molex 47219-2001, 4-bit, VDD switched by a TPS22918 from
  `SD_PWR_ON`, 10 kΩ pull-ups on CMD and DAT0–3, PRTR5V0U2X ESD on all six
  lines. The socket has no card-detect switch.
- FFC: two Hirose FH12-22S-0.5SH(55) 22-pin connectors. CAM0 uses D0/D1/clock,
  DSI0 uses D0/D1/clock; D2/D3 are NC on these 2-lane ports. Both share
  SCL0/SDA0; `CAM_GPIO` drives the camera power-enable pin.
- GPIO: two 1x20 headers make the 2x20 Raspberry Pi header. J_GPIO1 is the
  odd pins, J_GPIO2 the even pins.
- RTC: PCF85063ATL, 32.768 kHz crystal, CR2032 holder. VDD is diode-ORed from
  +3V3 and the coin cell; I2C pull-ups are on the RTC VDD rail, as NXP
  recommends. CLKOE is tied high and CLKOUT is NC.
- Fan: 2-pin +5 V header with a Schottky flyback diode, always on.

## Parts and datasheets

| Part | Datasheet | Revision | What was taken |
| --- | --- | --- | --- |
| CM4 | https://datasheets.raspberrypi.com/cm4/cm4-datasheet.pdf | 2024 (Raspberry Pi) | Table 6 pinout pages 16–22; section 5.1 sequencing page 25; power 1.4 A typical, 600 mA on CM4_3.3V/1.8V |
| Hirose DF40C-100DS-0.4V(51) | https://www.hirose.com/en/product/p/CL0684-4033-4-51 | product page, 2026-10-01 | 100 positions, 0.4 mm pitch, 0.3 A per contact, 1.5 mm mated height |
| TI LMR33640 | https://www.ti.com/lit/ds/symlink/lmr33640.pdf | SNVSB99C | 3.8–36 V in, 4 A, 400 kHz, VREF 1.0 V, EN 1.231 V, PG open drain |
| TI LMR38010 | https://www.ti.com/lit/ds/symlink/lmr38010.pdf | SNVSBM3B | 4.2–80 V in, 1 A, VREF 1.0 V, PG open drain |
| Microchip USB2514B | https://ww1.microchip.com/downloads/en/DeviceDoc/00001692C.pdf | DS00001692C | 36-pin SQFN pin list, CFG_SEL table 5-1, 24 MHz crystal, 12 kΩ RBIAS |
| NXP PCF85063ATL | https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf | Rev. 7.3, 2026-07-14 | DFN2626-10 pinout, I2C, 0.9–5.5 V, 32.768 kHz |
| UDE RB1-125B8G1A | https://www.lcsc.com/datasheet/C363353.pdf | RB115260-00, 2015-11-04 | 10/100/1000BASE-T, 1:1, 4 pairs + one centre tap, through-hole |
| Same Sky UJ2-ADH-3-TH | https://www.cuidevices.com/product/resource/uj2-adh-th.pdf | 1.04, 2024-09-12 | dual stacked USB-A, 8 contacts + shield |
| Molex 47219-2001 | https://www.farnell.com/datasheets/2213824.pdf | SD-47219-001 | 8 contacts, hinge, no card-detect switch |
| Hirose FH12-22S-0.5SH(55) | https://www.hirose.com/en/product/p/CL0586-0532-7-55 | product page | 22 positions, 0.5 mm, 0.5 A |
| Diodes DMP3098L | https://www.diodes.com/datasheet/download/DMP3098L.pdf | Rev. 08-1 | −30 V, ±20 V Vgs, −3.8 A, 56 mΩ at −10 V |
| Keystone 1058 | https://www.keyelco.com/product.cfm/product_id/14028 | catalogue page | CR2032/CR2025 SMD holder |
| KDS DST310S | https://www.lcsc.com/product-detail/C93232.html | LCSC page | 32.768 kHz, 12.5 pF, SMD3215 |
| PTTC SMD1812P260TF/16 | https://www.lcsc.com/product-detail/C438899.html | LCSC page | 2.6 A hold, 16 V |

## Known export defects

- The corrected-source schematic has **one** KiCad ERC error: the
  `RTC_BAT` power flag (`#PWR2153`). The source adds
  `power "RTC_BAT" 3v`; export16 is the re-run. The 13 errors in the first
  pre-GbE schematic (QOD/VOUT, FFC MP, Q_LED, U4 XTALOUT/PWR_FLAG) are gone;
  those were schematic-layout defects, not netlist defects. See
  `agent-run/VERIFICATION_REPORT.md`.
- The custom power rails `+5V_AUX`, `RTC_VDD`, `CM4_3V3` and `CM4_1V8` have
  power flags in the schematic but no symbols in the embedded
  `cm4_carrier.kicad_sym`; KiCad reports 72 `lib_symbol_issues` warnings.
  This is an exporter limitation for non-standard rail names.
- No PCB was exported, so there is no top/bottom copper or 3D image.

## Not verified by Synth

Synth does not check any of the following, and this board has not been
powered, so none of it is measured:

- **Power-up sequencing.** The PG chain and the CM4 sequence are designed in,
  not simulated. No timing was measured. The CM4 datasheet requires +1.8V at
  least 1 ms after +3.3V and RUN_PG at least 10 ms after +1.8V; no part of
  that is checked by Synth.
- **HDMI, USB and Ethernet signal integrity.** The diff pairs are declared
  with 100 Ω (Ethernet/HDMI/CAM/DSI) and 90 Ω (USB) targets, but the router
  does not guarantee coupled routing, length matching or a reference plane.
  Synth's impedance estimate is a closed-form approximation, not a field
  solver. No eye diagram, no TDR, no compliance test.
- **EMC.** No radiated or conducted emissions, no ESD gun test, no surge test.
  The TVS and ferrite bead are selected from datasheets, not measured.
- **Thermal.** The LMR33640 at 4 A and the LMR38010s at 1 A have exposed pads
  on GND copper with via-in-pad, but no thermal simulation or measurement was
  done. The 4 A figure is the regulator rating, not a measured board limit.
- **CM4 module compatibility.** The pin map is from Table 6, but no CM4 was
  fitted. The 0.3 A per-contact DF40 rating and the 1.5 mm mated height are
  from the Hirose page; the CM4 module's own tolerance was not measured.
- **USB hub configuration and enumeration.** The straps are set for
  self-powered, individual port power and over-current; no hub was enumerated.
- **microSD, FFC and RTC function.** No card, camera, display or battery was
  fitted. The FFC pinout is the Raspberry Pi 22-pin pinout, not a Synth check.
- **Battery life and RTC accuracy.** The CR2032 life and the 32.768 kHz
  accuracy are not calculated or measured.
- **Regulator loop stability, transient response and efficiency.** The
  component values follow the datasheet examples; no loop measurement or load
  transient was performed.

## Commands

```
export XDG_DATA_HOME=$(mktemp -d)
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
B=qualification/class-d/cm4-carrier
synth validate $B/cm4_carrier.synth                  # blocked: see QUALIFICATION.md
synth export-kicad $B/cm4_carrier.synth --out out/cm4 --allow-incomplete --router free-routing
kicad-cli sch erc --severity-all --format json -o $B/reports/cm4_carrier.kicad-erc.json $B/kicad/cm4_carrier.kicad_sch
kicad-cli pcb drc --severity-all --refill-zones --format json -o $B/reports/cm4_carrier.kicad-drc.json $B/kicad/cm4_carrier.kicad_pcb
synth check $B/cm4_carrier.synth --json
synth check $B/cm4_carrier.synth --fab --json
cargo test -p synth-cli --test cm4_carrier_board
```
