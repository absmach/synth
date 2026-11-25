# Multi-rail compute node with USB-C and a 5 to 12 V input (Class C, not a case-4 pass)

An STM32F103C8 microcontroller on four copper layers, with a 128 Mbit SPI flash and a USB-C receptacle.

- It is powered from a 5 to 12 V DC header or from USB VBUS.
- A 4 A buck converter, three linear rails, a load switch that sequences them and an expansion header complete the board.

It answers #331 under #40: a board whose thermal paths, assembly access and DFM limits are declared and visible in the exported output, made by an agent driving `synth mcp`. Jeff Mboya is the author of this pull request and has self-reviewed it. The independent PCB expert sign-off required by #40 is open.

## The board

| Group | Parts |
|---|---|
| DC input (5 to 12 V) | `J2` 1x2 header; `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 16 V); `D1` Littelfuse SMAJ15A TVS (15 V standoff above the 12.6 V maximum input at +5 %, 24.4 V clamp at 16.4 A, below the buck's 36 V input maximum); `D2` Vishay SS36 Schottky reverse-blocking (60 V, 0.75 V at 3 A) |
| USB input | `J1` Korean Hroparts TYPE-C-31-M-12 receptacle, flush on the left edge, 5 V only (D+ and D- go to the MCU); `R11` and `R12` 5.1 k CC pull-downs; `F2` Bourns MF-MSMF110/16-2; `D3` Nexperia PTVS6V0S1UR (6.0 V standoff above the 5.25 V maximum VBUS); `D4` Diodes 1N5819HW-7-F ORs VBUS into the buck input (40 V, 450 mV at 1 A); `D5` and `D6` Nexperia PESD5V0S1BA on D+ and D-; `D11` and `D12` the same part on CC1 and CC2; `C30` |
| Buck 3.6 V | `U1` Texas Instruments LMR33640ADDA (3.8 to 36 V in, 4 A, internal compensation, EN tied to VIN); `L1` Bourns SRN6045TA-6R8M 6.8 uH (Isat 5.7 A, Idc 3.6 A); input `C1`, `C2` 10 uF 25 V and `C3` 220 nF; output `C4`, `C5` 22 uF 25 V; `C6` 100 nF bootstrap and `C7` 1 uF VCC bypass; the divider `R2` 10 k and `R3` 3.83 k from the 1.0 V feedback reference gives 1.0 V x (1 + 10 / 3.83) = 3.611 V; `R1` pulls PG up. The inductor ripple at 12 V in, 400 kHz is (12 - 3.611) x (3.611 / 12) / (6.8 uH x 400 kHz) = 0.93 A |
| Linear rails and sequencing | `U2` LP5907 (3.3 V), `U5` TPS22918 load switch, `U3` AP2112K-1.8 and `U4` TLV70012 (1.2 V); see the notes below the table |
| Compute core | `U6` STMicroelectronics STM32F103C8T6 (LQFP-48) with decoupling `C20` to `C25`; `Y1` JYJE 3TJ480000FYLBC 8 MHz crystal with `C26`, `C27` 10 pF; reset `R9`, `C28`, `SW1`; `R10` pulls BOOT0 low; `R13` 1.5 k USB D+ pull-up; `J3` 1x5 SWD header |
| Memory | `U7` Winbond W25Q128JVSIQ, SOIC-8 208 mil, on SPI1 (`PA4` to `PA7`) with `C29`, `R14`, `R15` |
| Indicators, expansion and fabrication | `D7` red (3.6 V), `D8` green (3.3 V), `D9` and `D10` yellow user LEDs with `R5` to `R8`; `J4` Harwin M20-9990846 1x8 expansion header (3.6 V gated, 3.3 V, 1.8 V, 1.2 V, two GPIOs, two grounds); test points `TP1` to `TP9`; fiducials `FID1` to `FID3`; two M2.5 mounting holes |

Notes on the linear rails and sequencing:

- `U2` is a TI LP5907MFX-3.3 (3.3 V, 250 mA) with `C8`, `C9`.
- `U5` is a TI TPS22918DBVR load switch (53 mohm at 3.3 V) with `C12`, `C13` (1 nF, rise time) and `C14`. The enable delay is `R16` 100 k x `C17` 100 nF = 10 ms.
- `U3` is a Diodes AP2112K-1.8 (600 mA, no minimum load) behind the switch, with `C10`, `C11`.
- `U4` is a TI TLV70012DDCR (1.2 V, 200 mA) enabled by the 1.8 V rail, with `C15`, `C16`.
- Sequence: 3.3 V first, then the switch passes 3.6 V to the 1.8 V regulator, then 1.8 V enables the 1.2 V regulator.
- The 1.8 V and 1.2 V rails feed only the expansion header `J4` and test points. The STM32F103C8 runs from 3.3 V alone, so the sequencing protects no device on this board.

The processor was picked from the registry's MCUs (ATmega328P, ESP32-C3 and WROOM-32, nRF52840, RP2350, STM32F103C8, STM32G030 variants).

- The board needs a USB full-speed device, a 72 MHz core and an external SPI flash on a hand-solderable 0.5 mm LQFP. The registry's `stm32f103c8` provides them.
- This pull request extends that entry with the `usb_dn` and `usb_dp` capability tags (and the differential USB pin types) on PA11 and PA12, and the `spi_cs` tag on PB12.
- The flash is the registry's 128 Mbit W25Q128 with the 208 mil outline the JVSIQ part has.
- The regulator is the DDC package that LCSC lists.


## Not verified by Synth

- **Junction temperature of `U1`:** it is not estimated, because the efficiency curve of the buck was not read.
- **Thermal array:** the six vias (2 x 3) are below the 4 x 3 TI recommends.
- **Courtyard fill:** it is 20.5 %, below the Class C target of 30 %.
- **Unused rails:** the 1.8 V and 1.2 V rails carry no load on the board.
- **Review:** the board has a self-review only. The independent PCB expert sign-off required by #40 is open.
