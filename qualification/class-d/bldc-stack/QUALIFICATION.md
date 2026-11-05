# Qualification matrix: BLDC two-board stack (SYN-408)

**Status: requires expert review. This is not a completed qualification result.**
No board was fabricated, routed or powered. Section A lists what Synth or KiCad
checked and the evidence. Section B lists what no tool here proves and an
SI/PI/RF, power-electronics or safety expert has to sign off. No expert has
reviewed this stack. The authored parts deliberately carry `reviewed_by = ""`,
so they remain unverified.

This is one board pair in Class D (#41). It carries the cross-board consistency
case: two sources, one mapping file, four negative variants.

## Reproduce

Run from the repository root with this branch's Synth. Use an empty
`XDG_DATA_HOME` so a user registry cannot shadow the shipped parts.

```
export XDG_DATA_HOME=$(mktemp -d)
export KICAD_SYMBOL_DIR=/usr/share/kicad/symbols KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints
scripts/setup-routing-engines.sh --freerouting-only
B=qualification/class-d/bldc-stack
synth validate $B/control.synth
synth validate $B/power.synth
cargo test -p synth-cli --test bldc_stack_board
cargo test -p synth-cli --test board_outline
kicad-cli sch erc --severity-all --format json -o /tmp/control.erc.json $B/kicad/control/bldc_control.kicad_sch
kicad-cli sch erc --severity-all --format json -o /tmp/power.erc.json $B/kicad/power/bldc_power.kicad_sch
cd $B && grep -v '^#' SHA256SUMS | sha256sum -c -
```

Tool versions used: KiCad 10.0.7, FreeRouting 2.4.1, OpenJDK 21.0.12.1,
Synth 0.1.1. The tool rows below were rerun on 2026-11-05 against the revised
control layout (`outline 60mm x 30mm`, mounting holes on the edge, J_B2B flush,
vertical Hall column). The FreeRouting adapter still fails before producing
copper in this environment (see A9); routing is therefore not evidence.

## A. Checked by tools

| # | Claim | Evidence | Result |
| --- | --- | --- | --- |
| A1 | The control source parses, resolves, lowers and passes Synth ERC | `synth validate control.synth` | Exit 0, 0 errors, 21 warnings: 10 `W-SYNTH-SUPPLY-002`, 5 `W-SYNTH-PART-UNVERIFIED`, 2 `E-SYNTH-SCHEM-005`, 1 each `W-SYNTH-THERMAL-001`, `E-SYNTH-POWER-004`, `W-SYNTH-ANOMALY-001`, `E-SYNTH-SCHEM-004` |
| A2 | The power source parses, resolves, lowers and passes Synth ERC | `synth validate power.synth` | Exit 0, 0 errors, 42 warnings: 22 `W-SYNTH-PART-UNVERIFIED`, 6 `E-SYNTH-SCHEM-005`, 5 `W-SYNTH-SUPPLY-002`, 5 `E-SYNTH-SCHEM-003`, 3 `E-SYNTH-SCHEM-001`, 1 `W-SYNTH-ANOMALY-001` |
| A3 | Every J_B2B pin is mapped exactly once | `cargo test -p synth-cli --test bldc_stack_board`, test `every_j_b2b_pin_is_matched_once` | `status: ok`, 20 total, 20 matched, 100 %, no diagnostics |
| A4 | A swapped mapping is rejected | `mappings_neg_swap.json` | `warning`, `E-SYNTH-MULTIBOARD-003` |
| A5 | A dropped mapping is rejected | `mappings_neg_drop.json` | `warning`, `E-SYNTH-MULTIBOARD-004` |
| A6 | A 12 V to 3.3 V exchange is rejected | `mappings_neg_voltage.json` | `warning`, `E-SYNTH-MULTIBOARD-003`/`005` |
| A7 | A swapped source pin is rejected | `control_neg_swap.synth` | `warning`, `E-SYNTH-MULTIBOARD-003` |
| A8 | Connector and mounting holes align | Sidecars, test `the_connector_and_mounting_holes_line_up` | J_B2B and H1-H4 are at identical coordinates on both boards, 0.000 mm and the same rotation |
| A9 | Copper is routed | `synth export-kicad --router free-routing`; `synth check control.synth` | **Failed.** `tools/freeroute_autoroute.py` exits 1 with a KiCad `PROPERTY_ENUM` assert before routing; the same failure occurs on the committed Class-B buck board. `synth check` returns `status: fail` at the DRC stage because the default `kicad-routing-tools` checkout is not configured. `release_ready: false`, `validation_failed` |
| A10 | The exported projects load in KiCad | `kicad-cli sch erc` and `kicad-cli pcb drc` on the delivered files | Control: 0 ERC errors, 0 warnings. Power: 0 ERC errors, 0 warnings. DRC on the un-routed control PCB: 0 errors, 97 unconnected pads, 0 warnings; no routed copper exists |
| A11 | Every authored part has a datasheet, an MPN and a real KiCad footprint | `synth registry qualify --part <id>` | All authored parts pass pin/pad, symbol, dimension and provenance checks; each is blocked on `E-SYNTH-QUAL-008` "part has no reviewer", which is intentional: `reviewed_by = ""` |
| A12 | The mapping file is the only cross-board source of truth | `interboard_mappings.json` and the test | The test loads that file; no pin mapping is duplicated in the sources |
| A13 | The netlist matches the source | `synth dump-ir` / the exported netlist | Control: 61 components, 40 nets. Power: 111 components, 53 nets. Net names and members are in `agent-run/NETLIST.md` |
| A14 | Hashes of the delivered files | `SHA256SUMS` | Generated locally with `sha256sum`; no CI golden set exists for this new board |
| A15 | The missing datasheets were fetched and read | `datasheets/`, Firecrawl 2026-11-03 | Phoenix 1715721/1715734 (17.5 A, 400 V III/2, 5.08 mm), LCSC C5116480/C30867 (2.5 A/contact), onsemi 2N3904 (40 V, 200 mA); LCSC C480516/C480520 found for the terminal blocks |
| A16 | The control board is exactly 60 x 30 mm with the holes on the edge | `cargo test -p synth-cli --test board_outline` | Edge.Cuts extremes are `(0, 0)` and `(60, 30)`; the mounting hole keeps its sidecar edge coordinate and no ground via sits in its corner; a sidecar part outside the rectangle is `E-SYNTH-PLACE-003` |

## B. Not proven: needs expert review

| # | Claim | Why it is not proven | Status |
| --- | --- | --- | --- |
| B1 | Gate-driver dead time, shoot-through and blanking | IR2101 has no internal dead time; the STM32 must insert it. No firmware exists and no rule checks it | Open |
| B2 | Power-loop inductance, phase-node ringing and EMI | No loop-area, snubber or EMI analysis; the router never ran, so no copper geometry exists | Open |
| B3 | Junction temperatures and copper area | Only the `W-SYNTH-THERMAL-001` estimate from declared figures. 2 oz outer copper and thermal vias are declared, not measured | Open |
| B4 | Current-sense accuracy, bandwidth and common-mode range | Gain 40 and the 1.6 kHz input filter are hand calculations; offset, shunt tolerance and PWM sampling are not measured | Open |
| B5 | Connector keying, stack height and 24 V creepage | The mapping is pin-to-pin; the mechanical test checks X/Y, not Z, keying or pollution degree | Open |
| B6 | Motor-side transients and back-EMF | The 80 V MOSFETs and 30 V TVS are chosen from datasheets; no motor, cable or avalanche test | Open |
| B7 | 12 V buck stability and AMS1117 thermal margin | Datasheet equations only; no bench measurement. The LDO warning at 107 C is a worst-case estimate | Open |
| B8 | Part review against manufacturer drawings | Authored parts have no reviewer by design. The registry seed parts used here were not re-reviewed in this branch | Open |
| B9 | Fabrication and assembly | No Gerbers were generated (routing failed), no fab quote, no 2 oz or via-in-pad confirmation | Open |
| B10 | Function on hardware | Nothing was powered, programmed or measured | Open |

Rows B1-B7 are the ones that decide whether this stack meets a Class D bar.
They are open, not closed.

## C. Issue acceptance checklist

| Issue item | Verdict | Where |
| --- | --- | --- |
| Both boards pass `synth check` independently | Partial: both `synth validate` exit 0 with 0 errors; routing/`synth check` fails on the shared FreeRouting environment | A1, A2, A9 |
| Multiboard export reports 100 % matched pins | Done: 20/20 | A3 |
| Three negative mapping cases fail | Done: swap, drop, voltage; plus a source swap | A4-A7 |
| Connector current table with derating | Done in `README.md`; 2.5 A contact, 50 % bracket, highest pin 220 mA | README |
| README "Not verified by Synth" names dead time, shoot-through, loop inductance, EMI, motor transients | Done | README |
| Deterministic reruns give identical hashes | Not done: no routed artifact, local hashes only | A9, A14 |
| `qualified: false`, human gates false | Done | `qualification.json` |

## D. Matrix row

`qualification/matrix.toml` carries the row for this pair: class "D", status
"experimental", `expect = "fail"` because the routing environment fails before
the source/DRC stages can be completed, and `expect_fab = "fail"` for the same
reason. The row is evidence of the attempt, not a pass.
