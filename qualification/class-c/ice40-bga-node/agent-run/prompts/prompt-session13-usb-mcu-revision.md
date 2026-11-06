Class C case 1, revision with a USB-C input and a companion MCU (Synth MCP)

You continue a finished board, the iCE40HX4K BGA node `ice40_bga_node` (issue #328 under #40). Its Synth source is `ice40_bga_node.synth` in your workspace; the exported board is routed and released as it stands. The exported board has a large empty area to the left of the FPGA U1. Add two functions in that area.

Follow only this prompt. The only tools are the Synth MCP and Read, Write and Edit, limited to this workspace. Do not invent tool results, part numbers, datasheet values or validation results. Work only through the Synth tools, never edit exported KiCad files, and where Synth does something wrong report it exactly instead of working around it. Never set allow_incomplete. You have at most 6 exports; export into a fresh directory each time (export_1, export_2, ...).

1. What to add
- A USB-C receptacle (registry usb_c_receptacle) that is the board's 5 V input in place of the 2-pin power header J1: 5 V from VBUS goes through the existing polyfuse, TVS and P-FET reverse stage (keep that order). CC1 and CC2 each get a 5.1 kohm pull-down to ground. The shield goes to ground. USB-C is a connector: it must be flush with a board edge like the other connectors.
- A companion MCU, the registry-compatible STM32F103C8T6 in `user-registry/stm32f103c8_usb.synth.toml` (LQFP-48, USB full speed), powered from the 3.3 V rail, with: every supply pin decoupled within a few millimetres, a reset button and its network, a 8 MHz crystal (registry xtal_8mhz_3225_4p) with load capacitors computed from the crystal's load capacitance and a stated stray capacitance (write the arithmetic), a 5-pin SWD header (registry header_1x5_swd), BOOT0 pulled low, and USB D+ and D- to the receptacle with 22 ohm series resistors.
- The MCU's role: it programs the SPI flash U2 over SPI and controls the FPGA configuration. Wire MCU SPI1 (PA4 NSS, PA5 SCK, PA6 MISO, PA7 MOSI) to the flash's CS, CLK, DO and DI; wire two MCU GPIOs to the FPGA CRESET_B and CDONE. The iCE40 bitstream is 135,183 bytes, larger than the MCU's 64 KB flash, so the flash U2 stays the bitstream store and the MCU only streams it from the host. Say this in REPORT.md.
- Keep every existing function of the board: FPGA, flash, rails, LEDs, FPGA header, test points, mounting holes.

2. Rules
- Parts that are not ball grids: declare `escape { off }` on the USB-C receptacle and on the MCU (LQFP-48), as `component U5: mcu "..." { escape { off } }`. Only U1 is escaped as a ball grid.
- Net classes: a USB net is critical for the return-via rule and every layer change of a fast or unclassed net needs a ground return via beside it. Declare every other net class `edge_rate slow`. Declare D+/D- as a differential pair with the USB class the language allows.
- Every part in a named group (add groups "USB INPUT" and "MCU"), no OTHER region. Placement: use placement_hint near for decoupling and the crystal; the new parts belong in the free area left of U1, and the board outline must not grow. Check the exported courtyard fill and the outline against the previous board (44 x 40 mm).
- Part packages are checked against datasheets: do not change a package to make routing easier.

3. Process
- Read ice40_bga_node.synth first. Edit it in place. Validate with synth_validate after each stage, then export.
- Report in REPORT.md: the design decisions, the crystal arithmetic, the net classes and why each is slow or fast, the measured result of each export (state, DRC errors, open nets, blocking reasons, outline and fill), every Synth defect or limitation hit, and what you could not do. Do not write a README.md.
