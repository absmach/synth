# iCE40HX4K SDR SDRAM node with a USB-C power input (Class C, case 2)

An iCE40HX4K FPGA in a 144-pin TQFP (0.5 mm pitch) with a 64 Mbit SDR SDRAM (ISSI IS42S16400J, TSOP-II-54), an SPI boot flash, a 1x6 programming header and three status LEDs, powered from a USB-C receptacle (5 V only), on six copper layers. It answers #329 under #40: a board whose memory interface is length- and impedance-sensitive, made by an agent driving `synth mcp`. The interface is a single-data-rate SDRAM driven by a soft controller in the FPGA fabric: there is no DQS strobe, no differential pair and no DDR claim. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request.

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` Korean Hroparts TYPE-C-31-M-12 USB-C receptacle (5 V from VBUS only, D+, D- and SBU unconnected) with `R2` and `R3`, 5.1 k pull-downs on CC1 and CC2; `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 2.2 A trip, 16 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff, 6.67 V minimum breakdown) above the 5.25 V maximum input; `Q1` Vishay SI2301CDS P-MOSFET reverse-polarity stage with `R1`, `C1` |
| 3.3 V | `U1` AMS1117-3.3 with 10 uF capacitors `C2` to `C5` |
| 1.2 V core | `U2` Texas Instruments TLV70012DDCR, TSOT-23-5, with `C6` and `C7`; the PLL supply `VCCPLL` through the ferrite bead `L1` (Murata BLM18AG601SN1D) and `C23` |
| FPGA | `U3` Lattice iCE40HX4K-TQ144, supply decoupling 100 nF `C10` to `C24` |
| SDRAM | `U4` ISSI IS42S16400J-7TLI (64 Mbit, 143 MHz, 3.0 to 3.6 V), decoupling `C30` to `C37`, six 22 ohm series resistors `RS1` to `RS6` at the FPGA end of the command, clock and control lines |
| Configuration | `U5` Winbond W25Q128JVSIQ, SOIC-8 208 mil, master SPI; `R10` and `R11` pull `CRESET_B` and `CDONE` up, `R12` the chip select, `R13` and `R14` the write-protect and hold pins; `J2` 1x6 programming header (SS_B, SCK, SDI, SDO, CRESET_B, GND) |
| Indicators and test points | `D2` red (3.3 V present), `D3` green (CDONE, buffered by the N-MOSFET `Q2` AO3400A because CDONE is open-drain), `D4` green user LED; test points `TP1` to `TP7`; two M2.5 mounting holes at opposite corners |

The iCE40HX4K-TQ144 has 107 user I/O (Lattice DS1040), the part model in [`registry/parts/fpga/ice40hx4k_tq144.synth.toml`](../../../registry/parts/fpga/ice40hx4k_tq144.synth.toml) is built from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of that datasheet. The memory pins sit on the FPGA's left side facing the SDRAM, in the order of the SDRAM's near pin row, so the data bus runs straight across with no crossings.

The qualification verdict, measurements, evidence and sign-off are in [`QUALIFICATION.md`](QUALIFICATION.md); how the board was made is in [`PROVENANCE.md`](PROVENANCE.md). Gerber content hash `e4663ac189`.
