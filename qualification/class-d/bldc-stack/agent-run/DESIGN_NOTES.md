# Design notes: BLDC stack (SYN-408)

## Why two boards and where the boundary sits

The control board carries everything that runs from 3.3 V and touches the
operator: MCU, Hall inputs, encoder, RS-485, SWD, LEDs. The power board carries
the 24 V / 10 A stage: reverse protection, TVS, bulk capacitors, three half
bridges, shunts, current-sense amplifiers and the 12 V buck. The only electrical
connection is J_B2B (2 x 10). This keeps the 10 A copper and the phase nodes off
the control board and leaves one connector to map and test.

## MCU choice: STM32G030K8, LQFP-32

- Six PWM outputs: TIM1 CH1/2/3 on PA8/PA9/PA10 and TIM3 CH1/2/3 on PB4/PB5/PB0.
  That is the six gate-drive signals the IR2101 pair-per-phase needs. The
  issue's alternative (RP2350) was not needed.
- USART1 on PB6/PB7 drives the RS-485 transceiver; PB8 is DE//RE.
- ADC1 channels on PA0/PA1/PA2 (ISENSE_U/V/W), PA7 (VBUS_SENSE) and PB1
  (12 V sense).
- Hall inputs PA4/PA5/PA6, encoder PA11/PA12, status PB2/PB3.
- The part is a reviewed registry entry. Its pin table was not re-read in this
  branch; the registry review is the evidence.

The consequence: PA14 is both SWCLK and BOOT0 on the LQFP-32. The boot button
pulls PA14 high through 1 k, so a debugger driving SWCLK low is not shorted. The
cost is that the button and SWD cannot be used as independent functions; that is
recorded in the README, not hidden.

## 3.3 V LDO: AMS1117-3.3, not LM1117-3.3

The registry's `lm1117_3v3` names `Regulator_Linear:LM1117-3.3`, but the
installed KiCad library has `LM1117DT-3.3`, not that name. The exported
schematic therefore warned `lib_symbol_issues` and the PWR_FLAG anchor for the
12 V rail could not be built. `ams1117_3v3` has a stock `AMS1117-3.3` symbol
and the same SOT-223 pinout, so the board uses it. Both are 800 mA / 15 V
parts; the drop from 12 V to 3.3 V is the real limit, not the current rating.

## Gate drive: three IR2101, not a DRV8301

A DRV8301-class three-phase driver would add current-sense amplifiers, a buck
and a SPI interface that this board already implements discretely. Three
IR2101s are the "3 x half-bridge drivers" option the issue allows: one per
phase, HIN/LIN from the MCU, HO/LO through 10 R, a 1N5819 bootstrap diode and a
100 nF bootstrap capacitor. IR2101 has no dead time; the MCU must insert it.
That is a firmware property and is explicitly "not verified by Synth".

## MOSFETs: BSC030N08NS5 (80 V, 100 A)

The issue suggests a 40 V / 60 A part. A 40 V part leaves little room for the
back-EMF of a 24 V motor and the phase-node overshoot; the 80 V part is a
drop-in SuperSO8 with 3.0 mOhm RDS(on) and a KiCad symbol. Six devices, three
half bridges. The gate threshold is 2.1-3.3 V, so the 12 V gate drive is well
above it. The SOA and RDS(on) figures are in the datasheet, not measured.

## Current sense: 5 mOhm shunt and MCP6022 gain 40

Each low-side source returns through a 5 mOhm 2512 shunt. At 10 A the shunt
drop is 50 mV and the loss is 0.5 W (the 2512 is assumed 1 W; the generic
registry part has no power rating in a datasheet). The MCP6022 is a non-inverting
amplifier: gain = 1 + 39 k / 1 k = 40, so 50 mV becomes 2.0 V. The 1 k + 100 nF
input filter has fc = 1.6 kHz; the 100 pF across the 39 k feedback adds a
41 kHz pole. MCP6022 GBW is 10 MHz, so the closed-loop band is about 250 kHz.
The spare channel compares ISENSE_W with 1.65 V (about 8.25 A) and pulls
FAULT_N low through a 2N3904.

## 24 V input: P-FET reverse block and SMBJ30A

IRF4905 (55 V, 20 mOhm) is the reverse-polarity switch. The 22 k/10 k divider
holds VGS near -16.5 V at 24 V; BZT52C15 clamps gate-source at 15 V during a
TVS clamp (48.4 V), which the divider alone would not survive. The SMBJ30A
stands off 30 V (above the 26.4 V maximum input) and clamps at 48.4 V. Two
470 uF 50 V electrolytics and 2 x 10 uF ceramics carry the 10 A ripple; 50 V
is 2.1x the 24 V rail (the issue asks for 1.5x).

No fuse is fitted. The 10 A limit is an external supply limit, and that is a
real safety gap, recorded in the README.

## 12 V buck: LMR38010

The issue offers `mp2307_buck` or `lmr33640`. The MP2307 is a 23 V part (24 V
input is over its rating) and the LMR33640 is a 38 V part (the SMBJ30A clamp at
48.4 V is over its rating). LMR38010 is rated 85 V absolute maximum and 4.2-80 V
operating, so it survives the clamp. It is also a reviewed registry part. The
feedback divider is 110 k/10 k for 12.0 V; R_RT 33 k sets 800 kHz; L 6.8 uH.

## Connector: 2 x 10, 2.54 mm, 2.5 A

The issue explicitly says `header_1x20` is not enough and a 2 x 10 part must be
authored. The plug is ZHOURI C5116480, the socket BOOMELE C30867; both list
2.5 A per contact. The mapping uses pins 1-20 one-to-one. The 24 V / 10 A
current does not cross this connector; only the gate signals, sense signals,
12 V and 3.3 V do (table in README). A 2.54 mm header is therefore adequate and
is the part the issue asks for.

## Stackup and placement

Four layers: 0.07 mm outer copper (2 oz), 0.035 mm inner, 0.2/0.9/0.2 mm FR-4.
The phase and ground copper is 2 mm on the outer layers; the inner layers are
ground and a pour. The stackup block is in both sources.

The placement is hand-pinned in the two sidecars. J_B2B and H1-H4 are at the
same coordinates on both boards, so the mechanical test can enforce 0.1 mm. The
control board is roughly 72 x 58 mm and the power board the same frame; the
issue's 60 x 50 mm is a guideline the terminal blocks and the 2 x 10 connector
do not fit. The placement was not optimized for loop area: the automatic placer
produced 86 x 62 mm (control) and 154 x 105 mm (power), so a hand layout was
used instead. A reviewer should move the input capacitors and the shunts closer
to their switching loops before calling the layout production-ready.

## Cross-board check design

`interboard_mappings.json` is the only place the 20 pin pairs are written. The
checker loads both sources, resolves each pin by number or name, compares the
pin names and the signal-net names, requires every connector pin exactly once,
and compares the inferred voltage domain of the two nets. It does not claim
voltage isolation; that was a documentation claim in the old tool, and the code
only ever produced `E-SYNTH-MULTIBOARD-001`.
