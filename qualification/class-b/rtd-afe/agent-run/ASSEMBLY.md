# ASSEMBLY: rtd-afe

> Historical, agent-written. Derived from the committed `kicad/bom.csv` (export from source 01a77d54), which predates the sourcing pins in `rtd_afe.synth` (C_D is now X7R C14663; L1 C98331; Y1 C2449814). Do not use for assembly until the BOM is regenerated. See `QUALIFICATION.md` for status.

Source: `kicad/bom.csv`, `kicad/pnp.csv`, `rtd_afe.synth`, README.md, QUALIFICATION.md. Nothing was sent to a fab house or assembled.

## Fabrication
- 4 layers, 1.5862 mm declared FR4 stackup (1.6 mm nominal), 40.1 x 44.2 mm rectangle, 53 footprints.
- Manufacturer profile `jlc-filled-via`; `requires via_in_pad`. One via-in-pad at C_DV.2 (16.5, 22.3) mm: order with filled and capped vias. JLCPCB acceptance of this process: not verified.
- Four 2.5 mm NPTH mounting holes at (6, 3.5), (39, 3.5), (6, 40.7), (39, 40.7) mm.
- Fine pitch: RP2350 QFN-60 0.4 mm, ADS124S08 VQFN-32 0.5 mm, USB-C 0.5 mm. Fab capability check: not verified.
- Gerbers and drills: `kicad/` (and `kicad/drill/`). Fab review of them: not verified.

## BOM (`kicad/bom.csv`)
| Refs | Value | Notes |
| --- | --- | --- |
| U1 | ADS124S08IRHBR | MPN only, no LCSC |
| U2 | RP2350A | LCSC C42411118 |
| U3 | REF5025AIDR | MPN only |
| U4 | W25Q128JVSIQ | LCSC C97521 |
| U5 | LP5907MFX-3.3 | LCSC C83437 |
| Y1 | 12 MHz, ECS-120-20-33-TR | 3.2 x 2.5 mm 4-pad; no LCSC |
| J1 | TYPE-C-31-M-12 | LCSC C165948 |
| J2 | PT100, 1x4 2.54 mm | LCSC C492403, MPN PZ254V-11-04P |
| J3 | SWD, 1x3 2.54 mm | no MPN |
| D1, D2 | ESD (generic TVS, SOD-323) | placeholder, no MPN or LCSC |
| L1 | 3.3 uH 0805 | generic |
| L2 | 600R@100MHz 0603 ferrite | generic |
| R_S1, R_S2, R_BIAS | 1k 0603, 0.1 % | tolerance column set |
| R_CC1, R_CC2 | 5.1k 0603 | |
| R_DP, R_DN | 27 0603 | |
| R_RST, R_RUN | 10k 0603 | |
| R_X | 1k 0603 | |
| R_AV | 33 0603 | |
| C_D | 100 nF 0603 C0G 50 V | |
| C_CM1, C_CM2, C_F | 10 nF 0603 C0G 50 V | |
| C_X1, C_X2 | 33 pF 0603 C0G 50 V | |
| C_AV1 | 330 nF 0603 X7R 10 V | |
| C_AV2 | 10 uF 0805 X7R 10 V | |
| C_DV, C_IO, C_IO1..C_IO4, C_DV1, C_DV2, C_FL | 100 nF 0603 X7R 10 V | |
| C_RO, C_RI, C_RV | 1 uF 0603 X7R 10 V | |
| C_AVF, C_VI | 4.7 uF 0603 X7R 10 V | |
| C_DVB | 4.7 uF 0805 X7R 10 V | |
| C_L1, C_L2 | 2.2 uF 0603 X7R 16 V | |
| H1..H4 | M2.5 NPTH | |

The BOM `description` field of the generic capacitors reads "X7R/X5R, any value". The dielectric column carries the real attribute (C0G for the filter and crystal caps). Assemblers must follow the dielectric, voltage and tolerance columns, not the description. Parts with no LCSC number (U1, U3, Y1, J3, D1, D2, all generic passives): sourcing not verified.

## Placement and orientation
- `kicad/pnp.csv` is the pick-and-place file. Its correctness against a fab's rotation convention: not verified.
- J2 and J3 are at y = 41.7 mm, 0.71 mm from the bottom edge; J1 centred on the top edge, 1.27 mm from it. Connector overhang, header orientation and mating fit: not verified.
- 34 reference designators are hidden on silk (list in QUALIFICATION.md); they remain in the BOM, PnP and schematic. No board name, revision or header pin function on silk.
- Crystal Y1 pads 1/3 are the crystal, pads 2/4 ground.

## Notes
- No test points are fitted.
- Release override: `--allow-unverified-parts` for `ecs_120_20_33_tr` (no part-level reviewer).
- Hand soldering of the exposed pads (U1 EP, U2 EP): not verified.
