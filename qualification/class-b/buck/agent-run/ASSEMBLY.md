# Assembly: buck rev A

Source: `../kicad/bom.csv` (27 rows plus header), `../kicad/pnp.csv`, `../kicad/gerbers/`, `../kicad/drill/`, `../buck.synth`, `../reports/release-fab.json`. The board has not been fabricated or assembled; nothing here was checked with a fab or a DFM tool.

## Board

4 copper layers (F.Cu, In1.Cu, In2.Cu, B.Cu), outline 70.99 x 33.99 mm, `manufacturer "jlc-filled-via"`, `requires via_in_pad`. Gerbers: 22 files plus `buck-job.gbrjob`. Drill: PTH 27 holes (21 x 0.3 mm vias, 4 x 0.95 mm and 2 x 1.0 mm connector pins), NPTH 2 x 2.5 mm mounting holes. Six vias sit in U1's exposed pad (via-in-pad): order with resin-filled and capped vias; not confirmed with a fab. Filled-via support at the intended fab: not verified.

## BOM (from `bom.csv`)

| Refs | Value | Footprint | MPN | LCSC |
|---|---|---|---|---|
| U1 | LMR38010SDDAR | HSOP-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.1mm | LMR38010SDDAR | none |
| L1 | 6.8uH | Inductor_SMD:L_Bourns_SRN6045TA | SRN6045TA-6R8M | none |
| D1 | SS36 | Diode_SMD:D_SMC | SS36 | none |
| D2 | SMBJ30A | Diode_SMD:D_SMB | SMBJ30A | none |
| D3, D4 | LED green | LED_SMD:LED_0603_1608Metric | LTST-C190KGKT | none |
| F1 | 1.1A PTC | Fuse:Fuse_1812_4532Metric | MF-MSMF110/33X | none |
| C1 | 47uF 50 V electrolytic | Capacitor_SMD:CP_Elec_8x10 | RVT1H470M0810 | none in the BOM column; "JLCPCB C2895739" is in the description |
| C2 | 10uF X7R 50 V | C_1206_3216Metric | none | none |
| C5, C6, C9 | 22uF X7R 25 V | C_1206_3216Metric | none | none |
| C3 | 220nF X7R 50 V | C_0603_1608Metric | none | none |
| C4 | 100nF X7R 16 V | C_0603_1608Metric | none | none |
| C7 | 56pF C0G 50 V | C_0603_1608Metric | none | none |
| R_UV_T | 1.37M 1 % | R_0603_1608Metric | none | none |
| R_UV_B, R_PG, R_FB_T | 100k (R_UV_B and R_FB_T 1 %) | R_0603_1608Metric | none | none |
| R_FB_B | 43.2k 1 % | R_0603_1608Metric | none | none |
| R_RT | 33k 1 % | R_0603_1608Metric | none | none |
| R_LED, R_PGLED | 470 | R_0603_1608Metric | none | none |
| J1 | JST B2B-XH-A | Connector_JST:JST_XH_B2B-XH-A_1x02_P2.50mm_Vertical | B2B-XH-A | none |
| J2 | JST B4B-XH-A | Connector_JST:JST_XH_B4B-XH-A_1x04_P2.50mm_Vertical | B4B-XH-A | none |
| H1, H2 | M2.5 NPTH | MountingHole:MountingHole_2.5mm | none | none |

Only C1 carries an LCSC reference, and only in its description. Generic passives have no MPN or manufacturer sheet. Availability, lifecycle and price: not verified (no supply-chain check recorded).

## Polarity and orientation

- D1 SS36: cathode pad 1, anode pad 2 (registry). Placed at rotation 180 in the sidecar. Cathode bar vs footprint pad 1: not verified.
- D2 SMBJ30A: cathode pad 1, anode pad 2 (registry). Cathode toward VIN.
- D3, D4: cathode pad 1, anode pad 2; matches the Lite-On datasheet (cathode marked on package) per QUALIFICATION.
- C1: pin 1 is +, following KiCad `CP_Elec_8x10`. Datasheet does not state the marking: not verified.
- J1, J2: pin 1 net per `PIN_MAPPING.md`; pin-1 polarity not stated in the JST catalogue. J2 is rotated 270 degrees.
- U1: pin 1 is GND; exposed pad 9 to be soldered to the GND pour.

## Pad and footprint differences recorded for the reviewer (QUALIFICATION B8)

SS36 and SMBJ30A pads sit wider apart than Vishay's reference; SRN6045TA pads are 0.55 mm longer inward; MF-MSMF110/33X pads are 1.125 mm long against Bourns' 1.68 mm; LTST-C190KGKT KiCad pads are 0.875 x 0.95 mm at +/-0.7875 mm (outer span 2.45 mm) against 0.8 x 0.8 mm and 2.3 mm. `mf_msmf110_33x` also carries finding E-SYNTH-QUAL-016 (footprint names a 3D model, `Fuse_1812_4532Metric.step`, that is not installed). Whether each difference is acceptable: not verified.

## Silkscreen

References are printed; LED values "PWR" and "PG FAULT" are on F.Fab only. No other silk text is recorded: not verified.

## Files for the fab

`kicad/gerbers/*.gbr`, `buck-job.gbrjob`, `kicad/drill/*.drl` (PTH, NPTH), `kicad/bom.csv`, `kicad/pnp.csv`. Hashes in `../SHA256SUMS`. No fab or assembly house has accepted them.
