import csv
rows = list(csv.DictReader(open("sourcing.csv")))
cols = list(rows[0].keys())
def find(fp, mpn):
    for r in rows:
        if r["footprint"] == fp and r["mpn"] == mpn: return dict(r)
    raise SystemExit("missing " + fp + mpn)
def clone(fp, mpn, value):
    r = find(fp, mpn); r["value"] = value; return r
new = [
 clone("D_SOD-123", "BZT52C3V3", "3.3V zener"),
 clone("JST_XH_S2B-XH-A-1_1x02_P2.50mm_Horizontal", "S2B-XH-A-1(LF)(SN)", "JST S2B-XH-A-1"),
 clone("JST_PH_S4B-PH-K_1x04_P2.00mm_Horizontal", "S4B-PH-K-S(LF)(SN)", "JST S4B-PH-K-S"),
 clone("PinHeader_1x05_P2.54mm_Vertical", "M20-9990546", "SWD"),
 clone("SW_SPST_PTS645Sx43SMTR92", "PTS645SM43SMTR92 LFS", "RESET"),
 clone("SOIC-8_3.9x4.9mm_P1.27mm", "24LC256-I/SN", "24LC256-I/SN"),
 clone("LED_0603_1608Metric", "LTST-C190KGKT", "GREEN"),
]
def row(value, fp, mfr, mpn, pkg, rating, ds, dist, order, link, note=""):
    d = dict.fromkeys(cols, ""); d.update(value=value, footprint=fp, manufacturer=mfr, mpn=mpn, package=pkg, rating=rating, datasheet=ds, distributor=dist, order_number=order, link=link, note=note); return d
new += [
 row("0.5A hold","Fuse_1812_4532Metric","Littelfuse","1812L050/60MR","1812","PPTC, hold 0.5 A, 60 V (LCSC listing); trip 1 A (LCSC listing)","","LCSC","C87383","https://www.lcsc.com/product-detail/C87383.html","datasheet not opened"),
 row("100","R_0805_2012Metric","UNI-ROYAL","0805W8F1000T5E","0805","100 ohm, 1 %, 125 mW, 150 V","","LCSC","C17408","https://lcsc.com/product-detail/_UNI-ROYAL-Uniroyal-Elec-_C17408.html",""),
 row("330","R_0603_1608Metric","UNI-ROYAL","0603WAF3300T5E","0603","330 ohm, 1 %, 100 mW, 75 V","","LCSC","C23138","https://www.lcsc.com/product-detail/Chip-Resistor-Surface-Mount_UNI-ROYAL-Uniroyal-Elec-0603WAF3300T5E_C23138.html","LCSC page showed out of stock on one snapshot and in stock on another"),
 row("4.7uF","C_0603_1608Metric","Samsung Electro-Mechanics","CL10A475KP8NNNC","0603","4.7 uF, +/-10 %, 10 V, X5R","","LCSC","C1705","https://www.lcsc.com/product-detail/C1705.html","no DC-bias curve opened"),
 row("47uF","CP_EIA-3528-21_Kemet-B","KEMET","T491B476K010AT","CASE-B 3528-21","47 uF, +/-10 %, 10 V, tantalum, ESR 1 ohm (LCSC datasheet title)","https://datasheet.lcsc.com/lcsc/KEMET-T491B476K010AT_C116617.pdf","LCSC","C116617","https://datasheet.lcsc.com/lcsc/KEMET-T491B476K010AT_C116617.pdf","product page not opened; datasheet link from search"),
 row("AMS1117-3.3","SOT-223-3_TabPin2","Advanced Monolithic Systems","AMS1117-3.3","SOT-223","3.3 V fixed LDO, 1 A (LCSC listing)","http://www.advanced-monolithic.com/pdf/ds1117.pdf","LCSC","C6186","https://www.lcsc.com/product-detail/C6186.html","datasheet URL from a Flux page, not opened; thermal limit still to be checked"),
 row("BME680","Bosch_LGA-8_3x3mm_P0.8mm_ClockwisePinNumbering","Bosch Sensortec","BME680","LGA-8 3.0 x 3.0 x 0.93 mm","supply 1.71 to 3.6 V, I2C and SPI (JLCPCB listing)","https://ae-bst.resource.bosch.com/media/_tech/media/datasheets/BST-BME680-DS001.pdf","LCSC","C125972","https://www.lcsc.com/product-detail/Pressure-Sensors_Bosch-Sensortec-BME680_C125972.html","Octopart flags the part NRND"),
 row("STM32G030F6P6","TSSOP-20_4.4x6.5mm_P0.65mm","STMicroelectronics","STM32G030F6P6","TSSOP-20","Cortex-M0+, 64 MHz, 32 KB flash, 8 KB RAM, 2.0 to 3.6 V (JLCPCB listing)","","LCSC (JLCPCB listing)","C724040","https://jlcpcb.com/partdetail/C724040","LCSC page not opened; datasheet to be taken from st.com"),
 row("PTVS6V0S1UR","Nexperia_CFP3_SOD-123W","Nexperia","PTVS6V0S1UR,115","SOD-123W","TVS, 400 W, standoff 6 V, clamp 10.3 V max (distributor listings)","","Farnell (CPC)","SC21458","https://cpc.farnell.com/nexperia/ptvs6v0s1ur-115/diode-tvs-uni-6v-400w-sod123w/dp/SC21458","no LCSC listing found; DigiKey listing was out of stock"),
 row("GPIO","PinHeader_1x12_P2.54mm_Vertical","Harwin","M20-9991246","1x12 THT 2.54 mm","3 A per contact (JLCPCB listing)","","JLCPCB parts","C5439732","https://jlcpcb.com/partdetail/Harwin-M209991246/C5439732","datasheet not opened"),
]
keys = {(r["value"], r["footprint"]) for r in rows}
out = rows + [r for r in new if (r["value"], r["footprint"]) not in keys]
w = csv.DictWriter(open("sourcing_c2.csv","w",newline=""), fieldnames=cols); w.writeheader(); w.writerows(out)
print(len(out), "rows")
