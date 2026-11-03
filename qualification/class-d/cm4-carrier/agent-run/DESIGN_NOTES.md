# DESIGN_NOTES — CM4 carrier (SYN-407)

Why the board is built the way it is, and where it departs from a literal
reading of the issue.

## Module interface

- **Two 100-pin DF40C receptacles, not one 200-pin part.** The CM4 has two
  100-pin plugs; the carrier has two receptacles. The issue names
  `hirose_df40c_100ds_0v4_51` as one 100-pin Tier-2 part. One part cannot
  carry the right electrical types for both halves (pad 77 is +5V on A and
  HDMI0_TX1_P on B), so the tree has a base part for pins 1–100 and a `_b`
  variant for pins 101–200. Both have the same MPN and footprint; the BOM
  shows two lines with the same part number.
- **Synthesized symbols, not a stock 100-pin symbol.** KiCad's only 100-pin
  generic connector symbol numbers its pins `a1`…`b50`, which does not match
  the DF40 footprint's pads 1–100. The registry part omits `kicad_symbol`, so
  the exporter synthesizes a 100-pin rectangle with names `p1`…`p100`. That
  is electrically correct and keeps the pad numbers aligned. The schematic
  is large and the symbol is generic; `PIN_MAPPING.md` carries the signal
  names.
- **143 used pins, 57 NC.** The unused list is in the `CM4 MODULE` notes and
  `PIN_MAPPING.md`. PCIe, HDMI1, CAM1 and DSI1 are not brought out; the
  microSD is 4-bit, so DAT4–7 are NC; reserved pins are NC.

## Power

- **LMR33640 for +5V.** The issue allows LMR33640 or LMR38010. The LMR38010
  is 1 A, so it cannot carry the CM4 plus four USB ports. The LMR33640 is
  4 A, 400 kHz, with PG. The 4 A rating is the part's, not a measured board
  limit; the 6.8 µH inductor is 3.6 A Irms, so the true continuous limit is
  lower.
- **LMR38010 for +3V3 and +1V8.** The issue allows an LDO or buck with PG if
  the chosen regulator lacks it. The LMR38010 is an existing reviewed part
  with PG and a stock symbol. It is a 1 A buck for a 0.27 A and a ~0 A load;
  the alternative (a 300 mA LDO with PG) would need a new Tier-2 part and
  would not improve the budget.
- **+5V_AUX through L_FB1.** The two small bucks are fed from a ferrite bead
  off +5V. This keeps the 5 V power-budget net's declared loads below 4 A,
  but the true load includes the bead branch: 3.77 A, 0.23 A margin. That is
  documented in `POWER_BUDGET.md`; it is not a hidden load.
- **Sequencing.** PG_5V -> U2.EN, PG_3V3 -> U3.EN, PG_1V8 to a test point.
  The CM4's own GLOBAL_EN/RUN_PG/nEXTRST sequence is separate. Synth has no
  sequencing check; this is designed from the datasheet, not verified.
- **Reverse polarity.** A P-FET (DMP3098L, −30 V, ±20 V Vgs) instead of a
  Schottky. At 12 V and ~1.75 A the drop is much lower than a diode; the
  ±20 V gate rating means no gate clamp is needed.
- **TVS.** SMAJ15A (15 V standoff, 24.4 V clamp) is below the LMR33640's
  38 V absolute maximum. SMBJ30A (48.4 V clamp) would not protect the
  LMR33640. The 12 V rail is 12.0 V nominal; the TVS does not conduct.

## USB

- **USB2514B, not TUSB2046B.** The USB2514B is a 4-port hub with individual
  port power and over-current, a 24 MHz crystal and an LCSC number
  (C633338). The TUSB2046B is a 4-port hub too but the USB2514B was the part
  with a checked LCSC page and a stock KiCad symbol.
- **Per-port TPS22918 + PRTR5V0U2X.** One load switch per port gives the hub
  individual power switching; one 2-line ESD array per port covers D+/D−.
  The TPS22918's QOD is tied to VOUT so the port rail discharges when off.
- **OCS_N tied high.** There is no current-sense device; the hub is told
  there is no over-current. The TPS22918 has no fault output. This is a
  deliberate simplification, not a protected port.

## Ethernet

- **UDE RB1-125B8G1A, not the Abracon ARJP11A or Pulse JXD6.** The issue
  asks for GbE. The ARJP11A is 10/100. The Pulse JXD6-0001NL was the first
  choice, but its datasheet and product page say **100BASE-TX**; it is not a
  GbE part. The UDE RB1-125B8G1A datasheet (RB115260-00) says
  10/100/1000BASE-T, has four transformer pairs (TD1–TD4) and one common
  centre tap, and has a stock KiCad symbol and footprint. It is through-hole,
  not SMT.
- **Centre tap to GND through 100 nF.** The UDE part has one common centre
  tap (R1). The CM4IO board reference ties the magjack centre tap to GND
  through 100 nF; this board follows that. The CM4 datasheet says a standard
  1:1 magjack is all that is needed.

## HDMI

- **HDMI0 only.** The issue says HDMI-A; the CM4 has two HDMI ports. HDMI1
  is NC to keep the pin count and board area down.
- **TXS0108E for DDC.** The issue names `txs0108e`. It is a push-pull
  translator and HDMI DDC is open-drain; 2.2 kΩ pull-ups are fitted on both
  sides. The part is used as the issue asks, but a TXS0102/TCA9406 would be
  the open-drain-correct choice. This is a design risk for the DDC bus.
- **CEC and HPD direct.** The CM4's HDMI pins are 5 V tolerant and internally
  pulled; the datasheet says they can connect directly to the connector.

## microSD, FFC, GPIO, RTC

- **microSD on CM4 Lite.** The socket is 4-bit and has no card-detect switch;
  DAT3/CD is DAT3. The CM4 non-Lite uses eMMC and does not bring the SD pins
  out, so this block is only useful on a CM4 Lite.
- **FFC pinout.** The 22-pin pinout is the Raspberry Pi 22-pin CSI pinout
  (Arducam/HwB): GND, D0, D1, clock, D2/D3, power-enable, LED-enable, SCL,
  SDA, 3V3. CAM0 and DSI0 are 2-lane, so D2/D3 are NC.
- **GPIO header.** Two 1x20 headers, J_GPIO1 odd and J_GPIO2 even. The
  physical pin map is the Raspberry Pi 40-pin map; all 28 CM4 GPIOs are
  brought out. GPIO26 is shared with the RTC INT.
- **RTC.** The CM4 has **no VBAT pin**. The issue's "diode to CM4 VBAT per
  datasheet" cannot be implemented: Table 6 has no VBAT/RTC pin. The board
  uses a standalone PCF85063ATL with a CR2032, diode-ORed from +3V3 and the
  cell. This is the CM4IO board's approach and the only datasheet-correct
  one. The RTC I2C is on SCL0/SDA0 and the INT is on GPIO26.
- **CLKOE high.** The PCF85063A CLKOE is tied to RTC_VDD, so CLKOUT is
  enabled but NC. Tying it to GND triggered a false Synth POWER-008 on the
  GND net; the current cost is microamps.

## Stackup and placement

- **4 layers, 1.6 mm.** 0.035 mm copper, 0.2 mm prepreg, 1.06 mm core,
  0.2 mm prepreg, 0.035 mm copper. The prepreg thickness is what the
  impedance estimate uses; the fab's actual prepreg may differ.
- **Placement sidecar, not auto-placement.** The automatic placer exits 1 on
  this board (185 components, ten edge-hard connectors). Every component is
  pinned in `cm4_carrier.placement.layout.toml` with `priority = "hard"`, so
  the exporter uses the sidecar. The arrangement is a shelf-packed first
  pass, not an optimized layout; the router has not accepted it.
- **Board size.** The sidecar keeps the large parts inside a 100 × 80 mm
  envelope; the exporter refits Edge.Cuts around the placed courtyards, so
  the final outline is whatever the placement produces. It was not measured
  because no PCB exported.

## Pinout gaps (read this before trusting the netlist)

- **UDE RB1-125B8G1A pair order.** The KiCad symbol names TD1–TD4 on
  R2–R9 and the common centre tap on R1. The board maps CM4 Pair0–3 to
  TD1–TD4 in order. The UDE datasheet is a drawing; the pair order was read
  from the symbol names, not traced through the transformer drawing.
- **24 MHz crystal pad map.** The ECS ECX-33Q datasheet says pad 1/3 are the
  crystal and pad 2/4 are GND. The first revision had pad 2 as the second
  crystal electrode; the part now declares 1/3 crystal and 2/4 GND.
- **Molex 47219-2001 pin numbers.** The KiCad `Micro_SD_Card` symbol names
  DAT2/DAT3/CMD/VDD/CLK/VSS/DAT0/DAT1 on pads 1–8. The Molex SD-47219-001
  drawing was not read pin by pin; the mapping is the standard microSD
  mapping, not a datasheet-confirmed one.
- **Same Sky UJ2-ADH-3-TH pin numbers.** The KiCad `USB_A_Stacked` symbol
  names VBUS/D−/D+/GND for ports 1 and 2 on pins 1–8. The CUI/Same Sky
  drawing was not read pin by pin.
- **CM4 DF40 pad numbering.** The DF40C-100DS footprint pads 1–100 are
  assumed to be the CM4's pin numbers in order. The Hirose drawing and the
  CM4 mating plug were not cross-checked; this is the single highest-risk
  assumption on the board.
- **HDMI-A pin numbers.** The existing `hdmi_a_receptacle` part and its
  KiCad symbol were reused; the Amphenol drawing was not re-read.

## What was not done

- No schematic or PCB export completed; the schematic layout runs for more
  than 30 minutes at 97 % CPU on this board. This is a Synth capability
  limit, not a design choice.
- No routing, DRC, ERC-in-KiCad, Gerber, BOM or pick-and-place artifact.
- No hardware, no firmware, no measurement.
