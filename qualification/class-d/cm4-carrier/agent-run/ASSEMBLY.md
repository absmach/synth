# ASSEMBLY — CM4 carrier (SYN-407)

Not assembled. This is the assembly intent, not a build record.

## BOM summary

- 185 placed components, 2 connectors of 100 pins, 4 mounting holes.
- SMD: 0402/0603/0805/1206 passives, HSOIC-8 buck, two HSOIC-8 bucks,
  QFN-36 hub, DFN-10 RTC, SOT-23 P-FET and load switches, SOT-143 ESD arrays,
  SMC/SMB/SMA diodes, 3225 crystals, DFN/QFN exposed pads.
- Through-hole: two stacked USB-A, two 1x20 GPIO headers, 1x3 VREF header,
  1x2 boot header, 1x2 fan header, tactile switch.
- Connectors: two DF40C-100DS (0.4 mm), one RJ45 magjack, one HDMI-A, one
  microSD socket, two 22-pin 0.5 mm FFC, one CR2032 holder.

## Polarity and orientation

- **D1 SMAJ15A**: cathode to `VIN_PROT`, anode to GND. The band is the
  cathode.
- **Q1 DMP3098L**: source to `VIN_FUSED` (pin 2), drain to `VIN_PROT`
  (pin 3), gate to GND through R_G1 (pin 1). The SOT-23 pin 1 is the gate.
- **D_PWR, D_3V3, D_ETH, D_ETH2, D_PWR_CM4, D_ACT**: cathode to the net that
  sinks the current (GND for the power LEDs, the CM4 open-drain pin for the
  activity LEDs). The 0603 LED cathode is pad 1 in the KiCad footprint.
- **D_RTC1, D_RTC2, D_FAN**: Schottky anodes to the source (+3V3, coin cell +,
  fan low side), cathodes to `RTC_VDD` or +5V. The band is the cathode.
- **U1/U2/U3**: pin 1 is GND/PGND, pin 9 is the exposed pad. The exposed pad
  is soldered to the F.Cu GND pour with via-in-pad.
- **U4 USB2514B**: pin 1 is the first downstream D−; the exposed pad is VSS.
- **U12 PCF85063ATL**: pin 1 OSCI, pin 10 VDD, exposed pad to GND.
- **J_CM4A/J_CM4B**: the DF40 receptacle is keyed; the module plugs into both.
  The two connectors are the same part; the B instance is a separate registry
  part only because its pin types differ.
- **J_USB1/J_USB2**: stacked USB-A, pin 1 is port 1 VBUS; the shell tabs are
  through-hole.
- **J_ETH**: the magjack latch is at the board edge; the shield tabs are
  through-hole.
- **J_HDMI**: the mating face is at the board edge; the shell tabs are
  through-hole.
- **J_SD**: the card entry is at the board edge; the hinge is on the top side.
- **J_CAM/J_DSI**: bottom-contact FFC; the gold contacts face the board.
- **J_BT1**: the coin cell + is the top contact; the holder is SMD.

## Special fabrication needs

- **Via-in-pad, filled and capped.** The three buck exposed pads and the
  fine-pitch escapes need vias in pads. `manufacturer "jlc-hdi"` and `requires via_in_pad`; the board cannot be
  built on a process without resin-filled, capped vias and 0.1 mm laser
  microvias. `jlc-hdi` is the 6-layer HDI tier: 0.1 mm drill, 0.075 mm
  annular ring, 0.25 mm via, 0.1 mm track/clearance.
- **0.4 mm pitch DF40.** Two 100-pin connectors; the escape is the hardest
  geometry on the board. The CM4 connectors declare an `underpad` escape onto `In2.Cu`/`In3.Cu`;
  the 0.25 mm via in the 0.2 x 0.7 mm pad is the HDI feature that makes it
  possible. The fab drawing must state IPC-4761 Type VII (filled + capped +
  plated) for these vias.
- **0.5 mm pitch FFC and DFN/QFN.** Two 22-pin FFC connectors, a QFN-36 hub
  and a DFN-10 RTC. Stencil aperture and paste volume are not specified here.
- **6-layer 1.6 mm stackup.** The `.synth` declares F/B signal, In1/In4
  ground planes and In2/In3 signal; 0.035 mm copper and 0.2 / 0.33 / 0.33 /
  0.33 / 0.2 mm insulators. The fab's actual prepreg may differ; the
  impedance targets are estimates.
- **Through-hole connectors.** The USB-A, headers, tactile switch and the
  RJ45/HDMI shield tabs need wave or hand soldering if the assembly house
  only does SMD reflow.
- **Coin cell.** The CR2032 is not installed at reflow; it is a user-fit
  battery.

## Assembly order (intended)

1. SMD passives and ICs on the top side.
2. Reflow, inspect the DF40, QFN and DFN joints.
3. Through-hole connectors and headers; wave or hand solder.
4. Program/inspect nothing: there is no firmware on this board.
5. Fit the CR2032 last, after the 3.3 V rail has been checked for a short.
