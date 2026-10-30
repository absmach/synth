# Assembly documentation — protected_power_entry rev A

What the tools wrote: `export/bom.csv` (84 parts), `export/protected_power_entry.kicad_pcb`, `.kicad_sch`, `.kicad_pro`.
What they did **not** write: Gerber layers, drill files, pick-and-place / centroid file. `synth_export` does not produce them
(its own description says so), and no shell was available to run `kicad-cli`. Generate them from the KiCad project:
`kicad-cli pcb export gerbers`, `export drill`, `export pos`. Placement data below is from the sidecar
`protected_power_entry.placement.layout.toml` (board mm, origin top-left; centre of the footprint origin).

## BOM summary (exact identities; full list in `export/bom.csv`)
| Ref | Value | MPN | LCSC (registry-provided, unchecked live) | Footprint |
|---|---|---|---|---|
| J1, J3, J4, J5 | JST XH side-entry 2-pin | S2B-XH-A-1(LF)(SN) | C163035 | JST_XH_S2B-XH-A-1_1x02_P2.50mm_Horizontal |
| J2 | 1x8 header 2.54 mm | none (generic) | none | PinHeader_1x08_P2.54mm_Vertical |
| F1, F2, F3 | 1.5 A hold PPTC | 1812L150/16DR (Littelfuse) | C207070 | Fuse_1812_4532Metric |
| Q1 | P-FET -20 V | SI2301CDS-T1-GE3 (Vishay) | C10487 | SOT-23 |
| D1 | 400 W TVS 15 V | SMAJ15A (Littelfuse) | **none** | D_SMA |
| D9, D10 | 5.1 V zener | BZT52C5V1-TP | C78728 | D_SOD-123 |
| D11, D12, D13 | 3.3 V zener | BZT52C3V3 | **none** | D_SOD-123 |
| D8 | Schottky | 1N5819 per registry (identity inconsistent with SOD-123, see below) | C8224 | D_SOD-123 |
| D2 white, D3 blue, D6 amber | 0603 LED | **none** | **none** | LED_0603_1608Metric |
| D4 green | 0603 LED | LTST-C190KGKT (Lite-On) | none | LED_0603_1608Metric |
| D5 green | 0603 LED | 0603-GREEN | C72043 | LED_0603_1608Metric |
| D7 red | 0603 LED | LTST-C190KRKT (Lite-On) | none | LED_0603_1608Metric |
| U1 | MP2307 buck | MP2307DN-LF-Z (MPS) | C18921 | SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm |
| U2 | 3.3 V LDO | AMS1117-3.3 | C6186 | SOT-223-3_TabPin2 |
| U3 | load switch | TPS22918DBVR (TI) | C131941 | SOT-23-6 |
| L1 | 10 uH power inductor | SRN6045TA-100M (Bourns) | **none** | L_Bourns_SRN6045TA |
| C1, C10, C17 | 100 uF 25 V electrolytic, 6.3 mm | **none** | **none** | CP_Radial_D6.3mm_P2.50mm |
| C2, C3 | 10 uF 25 V X7R 1206 | none | none | C_1206 |
| C8, C9, C14 | 22 uF 10 V 1206 | none | none | C_1206 |
| C12, C15, C16 | 10 uF 10 V 1206 | none | none | C_1206 |
| C4, C6, C18-C21 | 100 nF 0603 | none | none | C_0603 |
| C5 | 10 nF 0603 | none | none | C_0603 |
| C7, C13 | 2.2 nF 0603 | none | none | C_0603 |
| R1 47 k, R2 10 k, R3 44.2 k, R4 10 k, R5 100 k, R6 33 k, R7 100 k, R8 1 k, R9 100, R10 10 k, R11 2 k, R12 1 k, R13 10 k, R14 10 k, R15 1 k, R16 10 k, R17 10 k, R18 1 k, R19 3 k, R20 3.3 k, R21 1 k, R22 1 k, R23 470, R24 3.3 k | 0603, 1 % (3 and 44.2 k need E96 for exactness) | none | none | R_0603 |
| TP1-TP10 | 1.5 mm pad | none | none | TestPoint_Pad_D1.5mm |
| H1-H4 | M2.5 NPTH | none | none | MountingHole_2.5mm |

Parts **without an LCSC number**: D1, D11-D13, D2, D3, D6, D4, D7, L1, C1-C21 (all), R1-R24 (all), J2, TP1-TP10, H1-H4.
Parts **without any manufacturer part number**: J2, D2, D3, D6, all capacitors, all resistors, TP1-TP10, H1-H4.
Capacitor voltage/dielectric and resistor tolerance are in the Value field only; the generic parts have no MPN.
Registry identity problems: D8 (1N5819 vs SOD-123 vs "SS14-class"); the registry LCSC numbers were not checked against a distributor (live lookup failed).

## Placement (sidecar, board mm)
J1 (9.7, 35) rot 270; F1 (27, 35); Q1 (37, 35) rot 180; R1 (37, 41); D9 (42, 41); D1 (44, 27); C1 (46, 46);
C2 (53, 28) 90; C3 (53, 35) 90; C4 (57, 28) 90; C5 (57, 38) 90; U1 (64, 34); L1 (64, 46);
C6 (70, 27) 90; R5 (74, 26) 90; R6 (74, 31) 90; R2 (71, 37); C7 (71, 40); R3 (78, 38) 90; R4 (78, 43) 90;
C8 (71, 49); C9 (76, 49); C10 (66, 56); U3 (84, 50); C12 (84, 56); C13 (90, 55); C14 (90, 46); R7 (84, 44); R8 (80, 44); D10 (79, 56);
U2 (66, 66); C15 (59, 65); C16 (74, 66); C17 (80, 68); C18 (70, 71);
F2 (86, 18); F3 (86, 30); J3 (100.5, 18) 90; J4 (100.5, 30) 90; J5 (100.5, 62) 90;
J2 (20, 69) rot 90; R9 (14, 62); R10 (24, 56); R11 (24, 61); R12 (28, 58); C19 (28, 61); D11 (32, 58); R13 (36, 56); R14 (36, 61); R15 (40, 58); C20 (40, 61); D12 (44, 58); R16 (46, 65); R17 (46, 70); R18 (50, 67); C21 (50, 70); D13 (54, 67);
D2 (20, 8); D8 (20, 13); R19 (20, 18); D3 (40, 8); R20 (40, 13); D7 (28, 8); R24 (28, 15); D4 (52.5, 53); R21 (52.5, 48.5); D5 (99, 46); R22 (99, 51); D6 (92, 69); R23 (92, 64.5);
TP1 (22, 26); TP2 (31, 27); TP3 (47, 36); TP4 (59, 52); TP5 (89, 40); TP6 (86, 68); TP7 (56, 46); TP8 (80, 61); TP9 (22, 44); TP10 (36, 70);
H1 (4, 4); H2 (109, 4); H3 (4, 71); H4 (109, 71).
The exporter's pre-placement positions (from `synth_place_with_hints`) are in nm; the placer snapped none of these (all hard overrides).

## Assembly notes
- Through-hole: C1, C10, C17 (radial, polarised), J2. All other parts SMD, single-sided (top).
- Hand-solder the electrolytic capacitors and J2 after reflow.
- Mating cables: JST XHP-2 housing with SXH-001T-P0.6 crimp contacts; wire 22-24 AWG for 1.5 A runs.
- U1's exposed pad must be soldered (reflow with paste; no thermal-via count is specified, so add vias in a review if needed).
- Silkscreen: reference designators and footprint marks only; no legends. Mark polarity of J1 by hand or add legends in KiCad before manufacture.
