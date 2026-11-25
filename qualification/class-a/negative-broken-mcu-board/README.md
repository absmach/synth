# Broken MCU board (Class A)

This is a 2-layer source-only test board with six declared components: a JST PH connector, an AMS1117-3.3 regulator, an STM32G030 MCU, a 100 nF capacitor declared twice and a resistor with no value. It has four planted defects. Design rationale is in the source comments, next to the circuits it explains.

The board is meant to fail. It shows what Synth reports when a design is wrong. It is useful only if the diagnostics name what is wrong and where. It is never routed and never exported, so it makes no claim about layout or fabrication.

No prompt was recorded for this board; see [`PROMPT.md`](PROMPT.md).

## The board

### Parts

| Group | Parts |
| --- | --- |
| Connector | J1: `jst_ph_2pin` |
| Regulator | U0: `ams1117_3v3`, value AMS1117-3.3 |
| MCU | U1: `stm32g030` |
| Capacitor | C1: `c_generic_0603`, 100nF, declared twice (planted defect) |
| Resistor | R1: `r_generic_0603`, no value (planted defect) |
| Source | `negative_broken_mcu_board.synth` |

### Layout

| Item | Value |
| --- | --- |
| Package | none: source-only test board |
| Copper layers | 2 (declared in the source) |
| Components | 6 declared in the source |
| Routing and export | Not done. If the board reached the router, the compiler would have let it through. |

### Checks

| Check | Result | Notes |
| --- | --- | --- |
| KiCad DRC (10.0.8, --refill-zones) | Not run | No package: the board is never exported. |
| KiCad ERC (10.0.8) | Not run | No package: the board is never exported. |
| `synth validate` | Exits 1 | The findings are error-level, so a script that gates on this board sees the failure. |
| Defect 1: U1.vss never connected | `E-SYNTH-CONNECT-001` | Required pin left unconnected. VDD is fed but VSS floats. |
| Defect 2: U1.nrst never connected | `E-SYNTH-CONNECT-001` | Required pin left unconnected. Together with defect 1: 2 findings. |
| Defect 3: C1 declared twice | `E-SYNTH-NAME-001` | Duplicate reference designator. Reported against each declaration: 2 findings. |
| Defect 4: R1 generic passive with no value | `E-SYNTH-VALUE-001` | 1 finding. |
| Error-level diagnostics in total | 5 | All five are reported in one run, so you do not have to fix one defect at a time. |
| Diagnostic location | Each finding names its component and pin or designator | Each finding points at a line in the source. |
| Failure stage | Semantic only | The board parses cleanly and lowers to IR. A syntax error would test the parser, not the rule engine. |
| `crates/synth-cli/tests/class_a_negative_board.rs` | Compares the error-level set with `synth validate --format json` | The comparison runs in both directions. A finding cannot vanish quietly, and a sixth cannot arrive undeclared. |

## Not verified by Synth

| Area | What is not verified |
| --- | --- |
| Impossible footprint | The fourth test case also asks for an impossible footprint. That defect is not on this board. A footprint KiCad does not ship is caught at export, not at validation, and this board never reaches export. The check `E-SYNTH-EXPORT-001` exists and several shipped parts trip it, but this board does not exercise it. |
| Warnings | The board also reports warnings. They are not part of the exercise and nothing asserts them. A new warning is free to appear. |
