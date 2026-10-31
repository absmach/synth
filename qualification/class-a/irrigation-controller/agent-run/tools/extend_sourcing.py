#!/usr/bin/env python3
"""extend_sourcing.py: add package, rating and datasheet columns to sourcing.csv.

Every figure below was read on 2026-10-29 from the manufacturer datasheet (Yageo, NIC,
NDK, Broadcom, AOS) or from the distributor page named in the row's `link` (LCSC, JLCPCB,
DigiKey, Distrelec, RS). Nothing is filled in from memory.
"""
import csv, sys
src, out = sys.argv[1:3]
D = {  # mpn -> (package, rating, datasheet)
 "CL10B104KB8NNNC": ("0603", "100 nF, +/-10 %, 50 V, X7R", "https://www.lcsc.com/datasheet/C1591.pdf"),
 "CL10B103KB8NNNC": ("0603", "10 nF, +/-10 %, 50 V, X7R", "https://www.lcsc.com/datasheet/C1589.pdf"),
 "CL10B222KB8NNNC": ("0603", "2.2 nF, +/-10 %, 50 V, X7R", "https://www.lcsc.com/datasheet/C16033.pdf"),
 "0603CG6R2C500NT": ("0603", "6.2 pF, 50 V, C0G; tolerance not on the page", "https://www.lcsc.com/datasheet/C1678.pdf"),
 "CL21A475KAQNNNE": ("0805", "4.7 uF, +/-10 %, 25 V, X5R", "https://www.lcsc.com/datasheet/C1779.pdf"),
 "CL21A106KAYNNNE": ("0805", "10 uF, +/-10 %, 25 V, X5R", "https://www.lcsc.com/datasheet/C15850.pdf"),
 "CL31A106KAHNNNE": ("1206", "10 uF, +/-10 %, 25 V, X5R", "https://www.lcsc.com/datasheet/C9807.pdf"),
 "CL31A226KPHNNNE": ("1206", "22 uF, +/-10 %, 10 V, X5R", "https://www.lcsc.com/datasheet/C5672.pdf"),
 "NRSZ101M25V6.3X11F": ("radial 6.3 x 11 mm, 2.5 mm pitch", "100 uF, +/-20 %, 25 V DC, -55 to 105 C, ripple 300 mA at 100 kHz and 105 C, 3000 h at 105 C", "https://static.rapidonline.com/pdf/11-1545.pdf"),
 "0603WAF1000T5E": ("0603", "100 ohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C22775.pdf"),
 "0603WAF6800T5E": ("0603", "680 ohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C23228.pdf"),
 "0603WAF1001T5E": ("0603", "1 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C21190.pdf"),
 "0603WAF2201T5E": ("0603", "2.2 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C4190.pdf"),
 "0603WAF4701T5E": ("0603", "4.7 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C23162.pdf"),
 "0603WAF1002T5E": ("0603", "10 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C25804.pdf"),
 "0603WAF4702T5E": ("0603", "47 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C25819.pdf"),
 "0603WAF1003T5E": ("0603", "100 kohm, +/-1 %, 100 mW, 75 V max, +/-100 ppm/C", "https://www.lcsc.com/datasheet/C25803.pdf"),
 "RC0603FR-079K1L": ("0603", "9.1 kohm, +/-1 %, 1/10 W, 75 V working, 150 V overload, +/-100 ppm/C, -55 to 155 C (Yageo RC_L datasheet V.14, Nov 2025; 9K1 = 9.1 kohm per its ordering key)", "https://yageogroup.com/content/datasheet/asset/file/PYU-RC_GROUP_51_ROHS_L"),
 "RC0603FR-0726K1L": ("0603", "26.1 kohm, +/-1 %, 1/10 W, 75 V working, 150 V overload, +/-100 ppm/C, -55 to 155 C (Yageo RC_L datasheet V.14, Nov 2025; 26K1 = 26.1 kohm per its ordering key)", "https://yageogroup.com/content/datasheet/asset/file/PYU-RC_GROUP_51_ROHS_L"),
 "1N5819HW-7-F": ("SOD-123", "Schottky, 40 V, 1 A, Vf 450 mV at 1 A, 550 mW, surge 25 A", "https://www.lcsc.com/datasheet/C82544.pdf"),
 "BZT52C3V3": ("SOD-123", "zener 3.3 V (3.1 to 3.5 V), 500 mW, leakage 5 uA at 1 V", "https://www.lcsc.com/datasheet/C545347.pdf"),
 "BZT52C5V1-7-F": ("SOD-123", "zener 5.1 V (4.8 to 5.4 V), leakage 2 uA at 1 V, junction -65 to 150 C", "https://www.lcsc.com/datasheet/C151588.pdf"),
 "NCD0603G1": ("0603", "green LED, 520 nm peak, Vf 2.6 to 3.6 V, 20 mA", "https://www.lcsc.com/datasheet/C84267.pdf"),
 "KT-0603Y": ("0603", "yellow/amber LED, 584 to 594 nm, Vf 2.4 V", "https://www.lcsc.com/datasheet/C2287.pdf"),
 "SMAJ13A": ("DO-214AC (SMA)", "TVS unidirectional, 13 V standoff, 15.9 V breakdown, 21.5 V clamp at 18.6 A, 400 W (10/1000 us)", "https://www.lcsc.com/datasheet/C148214.pdf"),
 "1812L150/16DR": ("1812 (4532 metric)", "PPTC, hold 1.5 A, trip 3 A, 16 V max, 100 A max, -40 to 85 C", "https://www.littelfuse.com/assetdocs/resettable-ptcs-1812l-datasheet?assetguid=CA5C80CB-504E-4A8A-8E74-0107520A1717"),
 "CBC3225T100MR": ("1210 (3225 metric)", "10 uH, +/-20 %, 900 mA rated and saturation, DCR 172.9 mohm", "https://www.lcsc.com/datasheet/C90315.pdf"),
 "AO3400A": ("SOT-23", "N-MOSFET, 30 V, 5.7 A, RDS(on) 48 mohm at 2.5 V, Vgs +/-12 V, 1.4 W", "https://www.lcsc.com/datasheet/C20917.pdf"),
 "MP2307DN-LF-Z": ("SOIC-8 exposed pad, 3.9 mm", "buck, 4.75 to 23 V in, 3 A, 0.925 V reference, 340 kHz, -40 to 85 C", "https://www.monolithicpower.com/en/documentview/productdocument/index/version/2/document_type/Datasheet/lang/en/sku/MP2307/document_id/503"),
 "NX3225GD-8MHZ-STD-CRA-3": ("3.2 x 2.5 x 0.8 mm SMD, 4-pin", "8.000 MHz fundamental, load 8 pF, max ESR 500 ohm, drive 10 uW typ and 200 uW max, -40 to 85 C", "https://mm.digikey.com/Volume0/opasdata/d220001/medias/docus/8882/NX3225GD-Datasheet.pdf"),
 "S2B-XH-A-1(LF)(SN)": ("through-hole, right angle, 2.5 mm pitch, 2 positions", "3 A, 250 V", "https://www.lcsc.com/datasheet/C163035.pdf"),
 "S3B-PH-K-S(LF)(SN)": ("through-hole, right angle, 2 mm pitch, 3 positions", "2 A, 100 V", "https://www.lcsc.com/datasheet/C157929.pdf"),
 "S4B-PH-K-S(LF)(SN)": ("through-hole, right angle, 2 mm pitch, 4 positions", "2 A, 100 V", "https://www.lcsc.com/datasheet/C157926.pdf"),
 "SI2301CDS-T1-GE3": ("SOT-23 (TO-236)", "P-MOSFET, 20 V, 3.1 A, 1.6 W, RDS(on) 142 mohm at 2.5 V, Vgs +/-8 V", "https://www.lcsc.com/datasheet/C10487.pdf"),
 "PTS645SM43SMTR92 LFS": ("SMD tactile switch, 6 mm", "SPST normally open, 50 mA at 12 V DC", "https://www.distrelec.ch/en/tactile-switch-50-ma-12-vdc-momentary-function-1no-57n-smd-pts-pts645sm43smtr92lfs/p/30119498"),
 "STM32G030K8T6": ("LQFP-32, 7 x 7 mm", "Cortex-M0+, 64 MHz, 64 KB flash, 8 KB SRAM, supply 2.0 to 3.6 V, -40 to 85 C", "https://www.lcsc.com/datasheet/C431631.pdf"),
 "APDS-9301-020": ("6-pin ChipLED", "I2C ambient light sensor, VDD 2.7 to 3.6 V, -30 to 85 C, I2C up to 400 kHz", "https://docs.broadcom.com/doc/AV02-2315EN"),
 "24LC256-I/SN": ("SOIC-8, 3.9 mm", "256 Kbit I2C EEPROM, 400 kHz, 2.5 to 5.5 V, -40 to 85 C", "https://www.lcsc.com/datasheet/C5458.pdf"),
 "AO3401A": ("SOT-23-3", "P-MOSFET, -30 V, -4.0 A at Vgs -10 V, RDS(on) under 50 mohm at -10 V, Vgs +/-12 V, -55 to 150 C", "https://aosmd.com/sites/default/files/res/datasheets/AO3401A.pdf"),
 "M20-9990546": ("2.54 mm pitch, 1 row, 5 positions, through-hole", "3 A per contact, contact resistance 30 mohm, -40 to 105 C", "https://azcus.digikey.com/en/products/detail/harwin-inc/M20-9990546/3728232"),
}
rows = list(csv.DictReader(open(src)))
miss = [r["mpn"] for r in rows if r["mpn"] not in D]
if miss: sys.exit(f"no rating row for: {miss}")
fields = ["value", "footprint", "desc", "manufacturer", "mpn", "package", "rating", "datasheet", "distributor", "order_number", "link", "note"]
with open(out, "w", newline="") as f:
    w = csv.DictWriter(f, fields); w.writeheader()
    for r in rows:
        p, rt, ds = D[r["mpn"]]
        w.writerow({**{k: r[k] for k in ("value", "footprint", "desc", "manufacturer", "mpn", "distributor", "order_number", "link", "note")}, "package": p, "rating": rt, "datasheet": ds})
print(len(rows), "rows written")
