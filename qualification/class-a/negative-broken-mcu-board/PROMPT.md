# Prompt

No prompt for this board was kept. This prompt is written from the finished board, from the planted defects listed in its source header, so that someone can rerun it.

Using synth mcp server write a deliberately broken two-layer board for JLCPCB that must fail validation and name each mistake. Use a JST PH 2-pin input J1, an AMS1117-3.3 regulator U0 and an STM32G030 MCU U1. Leave U1.vss and U1.nrst unconnected, which must raise E-SYNTH-CONNECT-001 for each. Declare C1 twice as a 100 nF 0603 capacitor, which must raise E-SYNTH-NAME-001, and add R1 as a generic 0603 resistor with no value, which must raise E-SYNTH-VALUE-001. Do not fix any of the four defects.
