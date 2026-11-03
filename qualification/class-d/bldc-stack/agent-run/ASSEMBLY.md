# Assembly: BLDC stack (SYN-408)

Written from the sources and the BOM. No board was assembled.

## BOM summary

| Board | Components | Through-hole | SMD | Notes |
| --- | --- | --- | --- | --- |
| Control | 61 | 4 mounting holes, J_B2B, J_HALL x3, J_ENC, J_SWD, J_RS485, SW_BOOT, H1-H4 | MCU, LDO, RS-485, LEDs, passives | 1.6 mm, 4 layers |
| Power | 111 | 2 x 470 uF, J_PWR, J_MOTOR, J_B2B socket, Q_REV/Q_GE TO-220, Q_GE_CTRL/Q_FAULT TO-92, H1-H4 | MOSFETs, gate drivers, op-amps, shunts, passives | 1.6 mm, 4 layers, 2 oz outer |

Full machine-readable BOM: `kicad/control/bom.csv`, `kicad/power/bom.csv`.
Sourcing notes and the missing LCSC numbers: `BOM_sourcing.md`.

## Orientation and polarity

- **Electrolytics C_BULK1/C_BULK2**: pin 1 is positive, on VBUS; pin 2 is
  ground. The KiCad `CP_Radial` symbol marks pin 1 with `+`.
- **AMS1117-3.3**: SOT-223 tab is pin 2 (VOUT), not ground. The tab copper is
  at 3.3 V; do not tie it to the ground pour.
- **LEDs**: pin 1 is the cathode, pin 2 the anode. D_LED_STATUS/D_LED_FAULT
  cathodes go to ground; anodes go to the resistors.
- **Diodes**: D_TVS cathode to VBUS; D_REV_Z cathode to the P-FET source
  (VBUS), anode to the gate; bootstrap diodes have the anode on +12V_GD and the
  cathode on VB.
- **MOSFETs**: BSC030N08NS5 drain is the large pad (pin 5) and the three source
  pads are pins 1-3. The gate is pin 4. The package marking is `030N08NS`.
- **IRF4905**: TO-220 pin 1 gate, 2 drain, 3 source. Q_REV drain is the 24 V
  input, source is VBUS; Q_GE source is +12V, drain is +12V_GD.
- **Connectors**: J_B2B pin 1 is marked on the plug/socket; the mapping is
  pin 1 to pin 1. J_PWR pin 1 is +24 V, pin 2 GND. J_MOTOR pin 1 U, 2 V, 3 W.
- **2N3904**: TO-92 pin 1 emitter, 2 base, 3 collector (the KiCad `TO-92_Inline`
  order). Check the part marking before inserting.

## Fabrication

- 4 layers, 1.6 mm, FR-4, 2 oz (0.07 mm) outer copper and 1 oz (0.035 mm)
  inner copper on the power board; the control board declares the same stackup.
- Minimum track/space in the sources: 0.2 mm on the control board and 0.3 mm on
  the power board. The phase and 24 V nets are 2 mm.
- No via-in-pad and no filled vias. The thermal vias under the MOSFETs are
  ordinary through vias in the drain copper; they will wick solder unless the
  fab tents them. A fab review is required.
- The 470 uF capacitors are 12.5 mm diameter with 5 mm lead spacing; the
  terminal blocks are 5.08 mm pitch. Both set the power board's height.
- The 2 x 10 connector is through-hole and is the only tall part on either
  board; a stacking height has not been specified.
- Silkscreen: the sources carry references and values; the fab panel is not
  checked. No fiducials or tooling holes are placed.

## What is not assembled or ordered

- No Gerbers, drill files or pick-and-place exist for a routed board; the
  routing stage failed before copper (see `VERIFICATION_REPORT.md`).
- No part was purchased. The two Phoenix terminal blocks are LCSC C480516/C480520; the generic passives
  still have no MPN.
- No stencil, paste or reflow profile was defined.
