# USB pair: supported speed, impedance estimate and skew

The USB data pair runs from `J1` through the 22 ohm series resistors `R25` and `R26` to `PA12` and `PA11` of `U5` (STM32F103C8). The delivered lengths are 34.3 mm (D+) and 44.2 mm (D-), a 9.9 mm difference. The pair is in the `USB` net class (0.12 mm tracks, 0.127 mm clearance, on `F.Cu` and `B.Cu`), declared 90 ohm differential.

## Supported speed

USB full-speed, 12 Mbit/s, only. The STM32F103x8/xB datasheet (DS5319) lists a USB 2.0 full-speed interface among its features and has a section "USB: Full-speed electrical characteristics" ([ST datasheet copies indexed at radiolocman.com](https://www.radiolocman.com/datasheet/pdf.html?di=184185)); the part has no high-speed PHY, so the board is not specified or tested for 480 Mbit/s.

## What full-speed asks of the pair

- Edge rates: USB transceiver data sheets give the full-speed rise and fall times as 4 to 20 ns, measured between 10 % and 90 % (onsemi/Fairchild USB1T20: [datasheet](https://www.mouser.com/datasheet/2/149/USB1T20-117246.pdf); TI TUSB1210: [datasheet](https://www.ti.com.cn/document-viewer/cn/lit/html/ZHCSKM0J/GUID-1003578D-34E0-47DB-BF8D-514B4C9072EC)). The bit time at 12 Mbit/s is 83.3 ns.
- Impedance: ST's application note AN4879 asks a full-speed platform to route 90 ohm +/-15 % differential and 30 ohm +/-30 % common mode ([AN4879, Rev 1 copy](https://github.com/PaxInstruments/labwiz-board/files/560836/ST.AN4879.pdf)). That is 76.5 to 103.5 ohm.

These are secondary copies of the figures (transceiver data sheets and an application-note copy), not the USB-IF specification text, which I did not read.

## Impedance estimate (analytic, not field-solved)

Edge-coupled microstrip on the outer layer of the declared stackup: dielectric height h = 0.1 mm, relative permittivity 4.4, track width w = 0.12 mm, copper t = 0.035 mm, gap s = 0.127 mm.

- Single-ended, Z0 = 87 / sqrt(er + 1.41) x ln(5.98 h / (0.8 w + t)) = 54.8 ohm.
- Differential, Zdiff = 2 x Z0 x (1 - 0.48 exp(-0.96 s / h)) = 94 ohm.

94 ohm is inside 76.5 to 103.5 ohm. The formulas are the common IPC-2141 approximations; they carry several ohms of error, the solder-mask and the real etched width are not modelled, and no field solver or TDR measurement was run.

## Skew

Effective permittivity 0.475 x 4.4 + 0.67 = 2.76 gives a propagation delay of sqrt(2.76) / 299.8 mm/ns = 5.5 ps/mm. The 9.9 mm difference is about 55 ps, 1.4 % of the 4 ns minimum rise time and 0.07 % of the bit time. At 12 Mbit/s that skew is far inside what the receiver and the 22 ohm series resistors absorb, so the pair is left unmatched; it would matter at 480 Mbit/s, which the part does not run.
