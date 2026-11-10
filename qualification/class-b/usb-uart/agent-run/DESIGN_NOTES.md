> **Retrospective documentation reconstruction, not reproducible prompt-to-PCB evidence.** The exact prompt, ordered tool calls, model and tool versions, and export session report were not saved. This board is **not Class-B qualified** (`qualified: false`). Interface limits: USB is bench-only (25.11 mm DP/DN skew, no coupled pair, no controlled impedance, no independent SI review); RS-485 is short-cable, point-to-point or last-node bench use only (no A/B surge protection, 120 ohm termination always fitted). The JLCPCB DFM record is for the previous revision and the final files were not re-uploaded.

Design notes reconstructed from the committed files. The board was built by an agent; `prompt.md` is the cleaned brief it was given (same text as the website showcase), not the exact text that was run. No tool-call log or report was saved.

# USB-C dual UART + RS-485 adapter (SYN-66): design notes

Sources: `../usb_uart.synth`, `../README.md`, `../qualification.json`, `../kicad/bom.csv`, `../reports/*.json`, `registry/parts/**`.
Status (README.md): design, placement and a FreeRouting route that passes Synth's release gate. Not fabricated, not tested on hardware. `qualification.json` has `qualified: false`.

## Intent
Bench or lab adapter, short cables, point-to-point or last node on an RS-485 bus. Not for industrial installs, long runs or a mid-bus node: A/B have no surge protection beyond the transceiver's built-in ESD and the 120 ohm termination is always connected (README.md).

## Architecture (from usb_uart.synth)
- J1 USB-C receptacle (TYPE-C-31-M-12), top edge. R1/R2 5.1k Rd on CC1/CC2.
- ESD: D1 PRTR5V0U2X on D+/D- (VCC pin on VBUS); D2 (VBUS), D5 (CC1), D6 (CC2) PTVS6V0S1UR.
- U2 LP5907 3.3 V LDO (C2/C3 2.2 uF), EN tied to VIN (always on). Feeds the +3V3 rail.
- U1 CH342F dual UART bridge (C4 VDD5, C5 V3, C6 VIO, each 100 nF).
- VIO rail is chosen by the J_VIO 3-pin header: pins 1-2 give +3V3, pins 2-3 give VBUS, no jumper leaves VIO unpowered and the bridge in reset.
- UART1 as TTL on J2 (GND, TXD1, RXD1) and J3 (DTR1, RTS1, CTS1); R5/R6 1k series on RXD1/CTS1.
- UART0 drives U3 THVD1450D RS-485 transceiver; header J4 (A, B, GND); R_TERM 120 ohm across A/B; DE and /RE tied together to DTR0/TNOW0; R_TNOW 4.7k to GND on TNOW0 (half-duplex mode).
- LEDs: D3 green VIO present; D4/D7 red, current from VIO via 470 ohm, sunk into TXD0/TXD1.
- Four M2.5 holes H1-H4. Two copper layers, 40.99 x 32.23 mm as routed, netclass "USB" 0.242 mm width / 0.127 mm clearance.

## Decisions recorded in the source
- VIO never below 3.3 V because the THVD1450 needs at least 3 V (usb_uart.synth header comment, README.md).
- Unused modem inputs RI0/RI1/DCD0/DCD1/DSR0/DSR1 pulled to VIO by R8; CTS0 held low by R9; RST left open (internal pull-up per README.md).
- USB pair has no impedance target: at 1.6 mm on two layers a 90 ohm pair needs about 1.18 mm traces (usb_uart.synth comment).
- Placement is pinned in `../usb_uart.placement.layout.toml`; J3 and J4 positions were moved 1 mm so the GND pour does not split and DTR1 routes (README.md).

## Known limits (README.md)
No VBUS bulk cap, fuse, reverse or inrush protection. No bias resistors on RS-485. No surge/TVS on A/B. No silkscreen text for header labels. R_TERM not selectable (no 2-pin header in the registry). Via-in-pad at U1 pin 25 needs fill and cap.

## Not verified
Everything on hardware; independent review of any part; datasheet values beyond what the part files and README record; availability and LCSC numbers (README.md "Not verified").
