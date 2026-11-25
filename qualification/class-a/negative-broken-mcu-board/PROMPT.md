# Design summary

No prompt for this board was kept. This summary is taken from the header of `negative_broken_mcu_board.synth`, which says the board is written to fail on purpose.

A two-layer STM32G030 board with a JST PH input and an AMS1117-3.3 regulator that must fail, and fail legibly: U1.vss and U1.nrst are never connected (E-SYNTH-CONNECT-001), C1 is declared twice (E-SYNTH-NAME-001), and R1 is a generic resistor with no value (E-SYNTH-VALUE-001). If the board ever passes, a rule has regressed.
