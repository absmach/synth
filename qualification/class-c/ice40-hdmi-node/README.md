# iCE40HX4K HDMI (DVI-style TMDS) node (Class C, case 3)

An iCE40HX4K FPGA in a 144-pin TQFP (0.5 mm pitch) driving an HDMI Type-A receptacle with four TMDS-style differential pairs (three data lanes and the clock) through 270 ohm source resistors, with the DDC lines level-shifted, a hot-plug-detect divider and a fused +5 V pin, an SPI boot flash and four status LEDs, powered from a 5 V terminal, on six copper layers. It answers #330 under #40: a board whose high-speed differential interface declares impedance, pair spacing and skew, with the delivered copper measured against those declarations. The interface is DVI-style: the iCE40 cannot source TMDS levels, so each line is an LVCMOS33 output through a series resistor into the sink's 50 ohm termination to 3.3 V. No HDMI compliance is claimed; the board demonstrates only what is measured below.

## The board

| Group | Parts |
|---|---|
| Power entry | `J1` JST B2B-XH-A 2-pin header (5 V in); `F1` Bourns MF-MSMF110/16-2 (1.1 A hold, 2.2 A trip, 16 V); `D1` Nexperia PTVS6V0S1UR (6.0 V standoff above the 5.5 V maximum input, 6.67 V minimum breakdown); `D2` Vishay SS36 Schottky (60 V, 3 A) as the reverse-polarity stage; `C1`, `TP1` |
| 3.3 V | `U1` AMS1117-3.3 with 10 uF capacitors `C2` to `C5` |
| 1.2 V core | `U2` Texas Instruments TLV70012DDCR, TSOT-23-5, with `C6` and `C7`; the PLL supplies `VCCPLL0` and `VCCPLL1` through the ferrite beads `L1` and `L2` and `C21`, `C22` |
| FPGA | `U3` Lattice iCE40HX4K-TQ144, supply decoupling 100 nF `C10` to `C20` and `C25` to `C27`; `X1` SiTime SiT8008 25 MHz oscillator with `C23` |
| Configuration | `U4` Winbond W25Q128JVSIQ, SOIC-8 208 mil, master SPI; `R30` and `R31` pull `CDONE` and `CRESET_B` up, `R32` and `R33` the write-protect and hold pins |
| HDMI | `J2` Amphenol 10029449-111RLF HDMI Type-A receptacle; `R1` to `R8` 270 ohm source resistors; `D3` onsemi SRV05-4 4-line ESD array on the DDC and HPD lines; `F2` Bourns MF-NSMF050-2 (0.5 A hold) on the +5 V pin; DDC level shifters `Q1` and `Q2` (IRLML6244) with `R9` and `R10` (2 k to +5 V) and `R11` and `R12` (10 k to 3.3 V); the HPD divider `R13` (10 k) and `R14` (20 k), which gives 5 V x 20 / (10 + 20) = 3.33 V at the FPGA |
| Indicators and test points | `D4` and `D5` green, `D6` and `D7` red LEDs with 470 ohm resistors `R20` to `R23`; test points `TP1` to `TP7` (+5 V, +3.3 V, +1.2 V, GND, CDONE, CRESET_B, +5 V_HDMI); two M2.5 mounting holes |

The iCE40HX4K-TQ144 has 107 user I/O (Lattice DS1040); the part model in [`registry/ice40hx4k_tq144.synth.toml`](registry/ice40hx4k_tq144.synth.toml) is built from KiCad's `FPGA_Lattice:ICE40HX4K-TQ144` symbol with the pin functions of that datasheet. The four lanes use the four adjacent pin pairs on the FPGA's left side (pins 1 and 2, 3 and 4, 7 and 8, 9 and 10), in the same top-to-bottom order as the connector's lanes, so no pair crosses another; the eight source resistors sit in one column between the FPGA and the connector.

The qualification verdict, measurements, evidence and sign-off are in [`QUALIFICATION.md`](QUALIFICATION.md); how the board was made is in [`PROVENANCE.md`](PROVENANCE.md). Gerber content hash `92d05ff69b`.
