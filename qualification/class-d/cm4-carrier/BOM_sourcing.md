# BOM sourcing — CM4 carrier (SYN-407)

This is the sourcing summary, not the generated BOM. The generated BOM is
`kicad/bom.csv` when the export completes; it did not complete, so this file
is the record. LCSC numbers were read from the LCSC/JLCPCB product pages on
2026-11-03. Stock was not checked at order time. "Alternate" means a part that
was looked up, not a part that was electrically or mechanically qualified.

## Authored / key parts

| Refdes | MPN | LCSC | Alternate | Datasheet | Notes |
| --- | --- | --- | --- | --- | --- |
| J_CM4A, J_CM4B | Hirose DF40C-100DS-0.4V(51) | C597931 | DF40C-100DS-0.4V(58) (LCSC not checked) | https://www.hirose.com/en/product/p/CL0684-4033-4-51 | 100-pin, 0.4 mm, 0.3 A/contact, 1.5 mm mated height |
| U4 | Microchip USB2514B-AEZC | C633338 | USB2514B-I/M2 (C220679) | https://ww1.microchip.com/downloads/en/DeviceDoc/00001692C.pdf | 36-pin SQFN 6x6 |
| J_USB1, J_USB2 | Same Sky UJ2-ADH-3-TH | C6731588 | UJ2-ADH-1-TH (LCSC not checked) | https://www.cuidevices.com/product/resource/uj2-adh-th.pdf | dual stacked USB-A, 8 contacts + shield |
| J_ETH | Pulse JXD6-0001NL | C5359047 | JXD0-0001NL (C5375613; different pinout) | https://productfinder.pulseeng.com/product/JXD6-0001NL | 1000BASE-T magjack, 1:1 |
| J_SD | Molex 47219-2001 | C164170 | Hirose DM3AT-SF-PEJM5 (LCSC not checked) | https://www.farnell.com/datasheets/2213824.pdf | 8 contacts, hinge, no card detect |
| J_CAM, J_DSI | Hirose FH12-22S-0.5SH(55) | C597981 | FH12-22S-0.5SH(59) (LCSC not checked) | https://www.hirose.com/en/product/p/CL0586-0532-7-55 | 22-pin, 0.5 mm, bottom contact |
| J_BT1 | Keystone 1058 | not found | QIJEY CR2032-BS-5-1 (C70379; footprint differs) | https://www.keyelco.com/product.cfm/product_id/14028 | CR2032/CR2025 SMD holder; LCSC number not found |
| U12 | NXP PCF85063ATL/1118 | C404360 | PCF85063AT/AAZ (C2157792; SO8, different footprint) | https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf | DFN2626-10, I2C RTC |
| Y2 | KDS DST310S 32.768 kHz | C93232 | NDK NX3215SA-32.768K-STD-MUA-8 (C156245) | https://www.lcsc.com/product-detail/C93232.html | 12.5 pF, SMD3215 |
| Y1 | ECS-240-12-33Q-JES-TR | not found | ECS-240-12-33Q (LCSC not checked) | https://ecsxtal.com/store/pdf/ECX-33Q.pdf | 24 MHz, 12 pF, 3225 4-pin; LCSC number not found |
| Q1 | Diodes DMP3098L-7 | C150492 | Vishay Si2301CDS (C10487; 20 V, lower rating) | https://www.diodes.com/datasheet/download/DMP3098L.pdf | −30 V, ±20 V Vgs, −3.8 A |
| F1 | PTTC SMD1812P260TF/16 | C438899 | Littelfuse 1812L300/24SLER (LCSC not checked) | https://www.lcsc.com/product-detail/C438899.html | 2.6 A hold, 16 V |

## Existing registry parts

| Refdes | Part id | MPN | LCSC | Datasheet |
| --- | --- | --- | --- | --- |
| U1 | `lmr33640` | LMR33640ADDA | not in registry | https://www.ti.com/lit/ds/symlink/lmr33640.pdf |
| U2, U3 | `lmr38010` | LMR38010SDDAR | not in registry | https://www.ti.com/lit/ds/symlink/lmr38010.pdf |
| U5–U8, U10, U11 | `tps22918_sot23_6` | TPS22918DBVR | C131941 | https://www.ti.com/lit/ds/symlink/tps22918.pdf |
| D_USB*, D_ETH_ESD*, D_HDMI*, D_SD* | `prtr5v0u2x` | PRTR5V0U2X | C12333 | https://assets.nexperia.com/documents/data-sheet/PRTR5V0U2X.pdf |
| U9 | `txs0108e` | TXS0108EPWR | C19187 | https://www.ti.com/lit/ds/symlink/txs0108e.pdf |
| J_HDMI | `hdmi_a_receptacle` | 10029449-111RLF | not in registry | https://www.amphenol-cs.com/product/10029449111rlf.html |
| D1 | `smaj15a_tvs` | SMAJ15A | not in registry | https://www.littelfuse.com/assetdocs/tvs-diode-sma-smb-smc-datasheet |
| D_PWR, D_3V3, D_ETH | `ltst_c190kgkt` | LTST-C190KGKT | not in registry | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KGKT.PDF |
| D_PWR_CM4, D_ACT | `ltst_c190krkt` | LTST-C190KRKT | not in registry | https://optoelectronics.liteon.com/upload/download/DS22-2000-074/LTST-C190KRKT.PDF |
| D_ETH2 | `led_amber_0603` | generic | not in registry | none |
| J1 | `jst_b2b_xh_a_2pin` | B2B-XH-A | not in registry | https://www.jst-mfg.com/product/pdf/eng/eXH.pdf |
| J_GPIO1, J_GPIO2, J_VREF, J_BOOT, J_FAN | `header_1x20`, `header_1x3`, `header_1x2` | generic 2.54 mm | not in registry | none |
| SW1 | `spst_tactile` | PTS645SM43SMTR92 LFS | C221880 | https://www.ckswitches.com/media/1479/pts645.pdf |
| H1–H4 | `mounting_hole_m2_5` | none | none | none |
| TP_* | `testpoint_pad_1_5` | none | none | none |
| L1–L3 | `l_bourns_srn6045ta_6r8m` | SRN6045TA-6R8M | not in registry | https://www.bourns.com/docs/Product-Datasheets/SRN6045TA.pdf |
| L_FB1 | `ferrite_bead_0603` | generic 600 Ω | not in registry | none |
| F1 alt, D_RTC1/2, D_FAN | `d_schottky` | generic | not in registry | none |
| R*, C* | `r_generic_0603`, `c_generic_0603/0805/1206`, `c_electrolytic_smd_8x10` | generic | not in registry | none |

## Sourcing gaps

- No alternate was electrically or mechanically qualified. The alternate
  column is a lookup, not a review.
- LCSC stock and lifecycle were not checked at order time.
- The Keystone 1058 and the 24 MHz ECS crystal have no LCSC number in this
  record. The QIJEY holder and the ECS-240-12-33Q are possible substitutes
  but their footprints differ.
- The CM4 connectors are the only 0.3 A/contact 100-pin DF40 parts; a
  different mated height changes the module stand-off and was not checked.
