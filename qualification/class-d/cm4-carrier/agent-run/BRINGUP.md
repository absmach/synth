# BRINGUP — CM4 carrier (SYN-407)

**Not run on hardware.** These are the intended first-power-up and functional
steps, with the measurements that would count as a pass. Every value is an
expectation from a datasheet, not a measurement.

## 0. Before power

1. Inspect for solder bridges on the DF40, QFN-36, DFN-10 and the 0.5 mm FFC
   connectors. Check the three buck exposed pads are wetted.
2. Check no short from `VIN_PROT`, `+5V`, `+3V3`, `+1V8`, `RTC_VDD` and
   `SD_VDD` to GND with a DMM (expect > 1 kΩ; the 22 µF banks may charge).
3. Do **not** fit the CM4, SD card, camera, display or CR2032.
4. Set the bench supply to 12.0 V with a 0.3 A current limit.

## 1. 12 V input and 5 V buck

| Step | Expected | Where |
| --- | --- | --- |
| Apply 12 V | input current < 30 mA with no load | J1 |
| `VIN_FUSED` | 12.0 V − PTC drop, > 11.8 V | F1 pad 2 |
| `VIN_PROT` | 11.9–12.0 V; Q1 fully on | D1 cathode |
| `+5V` | 4.95–5.05 V | TP_5V |
| `PG_5V` | > 4.5 V after the 5 V rail is good | TP_PG5V |
| `+5V_AUX` | 4.95–5.05 V (bead drop) | L_FB1 pad 2 |
| `+3V3` | 3.25–3.35 V | TP_3V3 |
| `PG_3V3` | > 3.0 V | TP_PG3V3 |
| `+1V8` | 1.77–1.83 V | TP_1V8 |
| `PG_1V8` | > 1.6 V | TP_PG1V8 |

If `+5V` is below 4.75 V, stop: the CM4 must not be fitted.

## 2. Sequencing

With a 4-channel scope on `VIN_PROT`, `+5V`, `+3V3`, `+1V8` and a fifth
channel on `PG_5V`/`PG_3V3`:

1. `+5V` rises monotonically to 4.75 V or more.
2. `PG_5V` goes high after `+5V` is good.
3. `+3V3` rises after `PG_5V`.
4. `PG_3V3` goes high after `+3V3`.
5. `+1V8` rises after `PG_3V3`.
6. `PG_1V8` goes high after `+1V8`.

The CM4 datasheet requires `+1.8V` at least 1 ms after `+3.3V` and RUN_PG at
least 10 ms after `+1.8V`. Measure the carrier rails first; the CM4 sequence
is measured in step 3.

## 3. CM4 fitted

1. Power down, fit the CM4 on the two DF40 connectors.
2. Apply 12 V. `+5V` should stay above 4.75 V; the CM4 idle current is about
   0.4 A.
3. Check `GLOBAL_EN` is high (internal 100k to +5V). Press SW1: `GLOBAL_EN`
   goes low, the CM4 powers down.
4. Check `RUN_PG` goes high at least 10 ms after `+1.8V`; `nEXTRST` goes high
   at least 1 s after `RUN_PG`.
5. Check `CM4_3.3V` at TP_CM4_3V3 is 3.2–3.4 V and `CM4_1.8V` at
   TP_CM4_1V8 is 1.75–1.85 V. These are module outputs, not carrier rails.
6. Check `GPIO_VREF` on J_VREF pin 2 is 3.3 V with the jumper 1–2. Move the
   jumper to 2–3 only if 1.8 V GPIO is intended.

## 4. USB

1. With no downstream device, the hub should enumerate on the CM4 as a
   4-port hub (`lsusb`).
2. Plug a USB 2.0 device into each port in turn; `lsusb` should show it.
3. Measure each port VBUS at 5.0 V ± 5 % with a 500 mA load.
4. Check `PRTPWR1–4` go high only for the enabled port.
5. Check `OCS_N1–4` stay high (no over-current).

## 5. Ethernet

1. Connect a GbE link. The CM4 should negotiate 1000BASE-T if the link partner
   supports it; this magjack is a 1000BASE-T part.
2. Check the link/activity LEDs (D_ETH, D_ETH2) are active low.
3. Run `iperf3` for a throughput smoke test. No eye diagram or compliance
   test is specified.

## 6. HDMI

1. Connect an HDMI sink. The CM4 should read EDID over DDC.
2. Check the TMDS pairs with a scope only if a sink fails to lock; no
   compliance test is specified.
3. Measure `HDMI_5V` at 5.0 V ± 5 % with the sink attached.

## 7. microSD, FFC, GPIO, RTC

- **microSD**: insert a card, check `SD_PWR_ON` goes high and `SD_VDD` is
  3.3 V, then mount the card. This only works on a CM4 Lite (the non-Lite
  module uses eMMC and does not bring the SD pins out).
- **FFC**: connect a Raspberry Pi 22-pin camera to J_CAM and a DSI display to
  J_DSI; check `SCL0`/`SDA0` idle high and the devices enumerate. The D2/D3
  lanes are not connected on these 2-lane ports.
- **GPIO**: check 3V3 on header pins 1/17, 5V on 2/4 and GND on
  6/9/14/20/25/30/34/39. Toggle GPIO2 and GPIO21 and measure the header.
- **RTC**: with the CR2032 fitted, read the PCF85063A over I2C at 0x51,
  set the time, remove 12 V, wait, re-apply and read it back. Check the INT
  output on GPIO26.

## 8. Fan

Apply 5 V to J_FAN with a small fan; check the fan runs and D_FAN clamps the
turn-off spike. The header is always on.

## What would count as a failure

- Any rail outside the table in step 1.
- `PG_5V`/`PG_3V3`/`PG_1V8` not asserting.
- The CM4 not booting, or `RUN_PG`/`nEXTRST` not following the sequence.
- A USB port that does not enumerate or whose VBUS collapses under 500 mA.
- A GbE link that does not come up, or an HDMI sink that cannot read EDID.
- A microSD card that is not detected on a CM4 Lite.
- An RTC that loses time with the 12 V removed.
