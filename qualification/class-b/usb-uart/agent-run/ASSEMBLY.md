# Assembly

Sources: `../kicad/bom.csv`, `../dfm/`, `../qualification.json` (`jlcpcb_dfm`), `../README.md`. Nothing fabricated or assembled.

## Board
2 layers, 1.6 mm default thickness (no declared stackup), 40.99 x 32.23 mm, manufacturer jlcpcb, revision A. Quote selection recorded for the earlier revision: FR-4 TG135, 1 oz outer copper, HASL with lead, green mask, white silkscreen, 5 boards; Economic assembly, top side.

## BOM (bom.csv: 36 refs; JLCPCB groups them into 28 lines, per qualification.json)
| Refs | Part | MPN / LCSC |
| --- | --- | --- |
| U1 | CH342F QFN-24 | CH342F / C2841530 |
| U2 | LP5907MFX-3.3 SOT-23-5 | C83437 |
| U3 | THVD1450D SOIC-8 | C2671361 |
| J1 | USB-C TYPE-C-31-M-12 | C165948 |
| D1 | PRTR5V0U2X SOT-143 | no LCSC in bom.csv |
| D2, D5, D6 | PTVS6V0S1UR | no LCSC in bom.csv |
| D3 | LTST-C190KGKT | no LCSC in bom.csv |
| D4, D7 | LTST-C190KRKT | no LCSC in bom.csv |
| J2, J3, J4, J_VIO | 1x3 2.54 mm header (through-hole) | none |
| C2, C3 | 2.2 uF X7R 0603 | none |
| C4-C7 | 100 nF X7R 0603 | none |
| R1, R2 | 5.1k 0603 | none |
| R3, R4, R7 | 470 0603 | none |
| R5, R6 | 1k 0603 | none |
| R8, R9 | 10k 0603 | none |
| R_TNOW | 4.7k 1% 0603 | none |
| R_TERM | 120 ohm 1% 0603 | none |
| H1-H4 | M2.5 NPTH mounting hole | none |

## Points to watch (qualification.json jlcpcb_dfm, README.md)
- The JLCPCB findings are for the previous board revision; the Gerbers and CPL in `../dfm/` were regenerated and not re-uploaded (stale, gate false).
- CPL covers SMD only: J2, J3, J4, J_VIO and H1-H4 are not placed by Economic assembly; headers are hand soldered.
- On upload JLCPCB matched 21 of 28 lines automatically; 7 were chosen by hand (D1 C12333, D2/D5/D6 C294622, U2 C80670, R_TERM C22787, R_TNOW C23162). D4/D7 auto-match was a yellow-green LED and was changed by hand to KT-0603R (C2286). D3 match VLMTG1401-GS08 (C511604) colour not checked. These picks are not component reviews.
- LED polarity: D4 cathode pad 1 is on TXD0 in KiCad; D7 not opened in KiCad; JLCPCB has not confirmed KT-0603R polarity. Rotation of D1, D2, D5, D6, U1, U2, U3 not checked.
- Via-in-pad at U1 pin 25 (EP): the fab must fill and cap; JLCPCB not asked.
- U1 exposed pad: KiCad footprint 2.6 mm, supplier drawing 2.4 mm, not reconciled.
- J1 courtyard is 0.01 mm from the top edge (shell face 0.50 mm inside Edge.Cuts); enclosure cutout fit not checked.
- Parts availability and registry LCSC numbers: not verified.

## Hand-fit parts and jumpers
Fit a jumper on J_VIO for 1-2 (3.3 V) or 2-3 (5 V). R_TERM is always connected. Screw size and standoff height: not chosen.
