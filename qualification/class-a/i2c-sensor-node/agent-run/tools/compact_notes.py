#!/usr/bin/env python3
"""compact_notes.py <in.synth> <out.synth>: replace each group's notes block of the case 2 source with the compact set below (pin maps, load budget, thermal and fault-current arithmetic); nothing but note text changes."""
import re, sys
NOTES = {
"5V INPUT: FUSE THEN DIODE": [
 "J1 pin 1 = +5 V, pin 2 = GND (JST XHP-2). Fuse first, then the reverse-polarity diode D1.",
 "D4 clamps transients on +5V_PROT, behind D1 so a reversed supply never forward-biases it."],
"3V3 REGULATOR": [
 "AMS1117 tab is VOUT. Load budget 100 mA: P = (5.0 - 3.3) x 0.1 A = 0.17 W, thetaJA 90 C/W gives a 15 C rise (125 C limit).",
 "LED (VF 1.9 to 2.4 V): 1.3 to 2.1 mA through 680 R. C3 is 47 uF tantalum (22 uF required)."],
"STM32G030 MCU": [
 "J4: 1 = 3V3_REF, 2 = SWDIO, 3 = SWCLK, 4 = GND, 5 = NRST. PA14 is SWCLK and BOOT0.",
 "D8, D9, D19 are 3.3 V zeners; a 5 V fault through 100 R is (5 - 3.3) / 100 = 17 mA, 56 mW."],
"I2C SENSING": [
 "BME680 SDO low = 0x76, CSB high = I2C. 24LC256 A2:A0 low = 0x50. No collision.",
 "One pull-up pair, 4.7k each, for the whole bus."],
"UART BREAKOUT": [
 "J3: 1 GND, 2 UART_TX, 3 UART_RX, 4 3V3_REF (3 mA max). Host logic 3.3 V to 5 V.",
 "D7, D24 are 3.3 V zeners behind 330 R and 1 k: a 5 V fault is 5.2 mA (17 mW) and 1.7 mA (5.6 mW)."],
"GPIO HEADER": [
 "J5: 1 3V3_REF, 2 GND, 3 PA0, 4 PA1, 5 PA4, 6 GND, 7 PA5, 8 PA6, 9 PA7, 10 GND, 11 PB0, 12 PB9.",
 "D10..D13, D20..D23: one 3.3 V zener per line behind 330 R; a 5 V fault is 5.2 mA, 17 mW. 3V3_REF is +3V3 through D14 and 100 R, 3 mA max."],
"MOUNTING": [
 "H2, H3: two M2.5 non-plated mounting holes at opposite (diagonal) corners."],
}
WIDTH = int(sys.argv[3]) if len(sys.argv) > 3 else 48
KEEP = int(sys.argv[4]) if len(sys.argv) > 4 else 99
t = open(sys.argv[1]).read()
def sub(m):
    name = m.group(2)
    import textwrap
    wrapped = [w for s in NOTES[name][:KEEP] for w in textwrap.wrap(s, WIDTH)]
    lines = "\n".join('      "%s"' % s for s in wrapped)
    return m.group(1) + 'notes "" {\n' + lines + '\n    }'
t2 = re.sub(r'(group "([^"]+)"[^{]*\{.*?)notes "" \{.*?\n    \}', sub, t, flags=re.S)
open(sys.argv[2], "w").write(t2)
print(len(t), "->", len(t2))
