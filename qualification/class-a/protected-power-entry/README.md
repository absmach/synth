# Protected power hub

A 12 V power-entry board: J1 takes 12 V through the polyfuse F1 (1.5 A hold), the SMAJ13A TVS D1 and the AO3401A reverse-polarity PFET Q1 (gate held by the 5.1 V zener D9 and R1). An MP2307 buck makes 5.0 V. A TPS22918 load switch makes a switched 5 V output, and an AMS1117-3.3 makes 3.3 V. Three JST XH outputs carry +5 V (J3), switched +5 V (J4) and +3.3 V (J5), and an 8-pin header J2 takes the load-switch enable from the outside and returns the 12 V, 5 V and 3.3 V sense levels. Six indicator LEDs show the rails and a tripped fuse, and ten test points reach the nodes. Two copper layers on JLCPCB, 114.9 x 76.0 mm, 84 parts. There is no microcontroller on the board: it is a power stage, powered from J1 alone.

The design is `protected_power_entry.synth` with its placement sidecar `protected_power_entry.placement.layout.toml`. It was made with `synth mcp` from a prompt, then exported through the Synth MCP server (the `synth_export` tool) and checked with KiCad. [`PROVENANCE.md`](PROVENANCE.md) is the record of that path: the exact prompt, the agent's tool-call trace, the Synth revision, the registry inputs, the artifact hashes and the validation commands with their results.

## What the board is

| Function | Parts | What it does |
|---|---|---|
| 12 V input and protection | J1 (JST S2B-XH-A-1), F1 (1.5 A hold polyfuse), D1 (SMAJ13A), Q1 (AO3401A), D9 (5.1 V zener), R1 (47 k), C1 (100 uF 25 V) | 12 V enters on J1. F1 limits fault current. D1 clamps surges on the fused node ahead of Q1. Q1 blocks a reversed supply; D9 holds its gate within 5.1 V of the source. |
| 5 V buck | U1 (MP2307), L1 (10 uH), C2 and C3 (10 uF 25 V), C4 (100 nF), C5 (10 nF), C6 (100 nF), R2 and C7 (compensation), R3 and R4 (feedback divider), R5 and R6 (enable divider), C8 and C9 (22 uF), C10 (100 uF) | Makes 5.0 V from the protected 12 V: 0.925 V x (1 + 44.2 k / 10 k) = 5.01 V. The enable divider starts the buck above about 4.8 V in. |
| Load switch | U3 (TPS22918), C12 (10 uF), C13 (2.2 nF), C14 (22 uF), R7, R8, D10 | Switches +5 V to +5V_SW. LOAD_EN is off by default (R7 pull-down) and turns the output on above 1.0 V. |
| 3.3 V regulator | U2 (AMS1117-3.3), C15 and C16 (10 uF), C17 (100 uF), C18 (100 nF) | Makes 3.3 V from the 5 V rail. |
| Outputs | J3, J4, J5 (JST S2B-XH-A-1), F2, F3 (polyfuses) | J3 pin 1 +5 V, J4 pin 1 +5V_SW, J5 pin 1 +3V3; pin 2 of each is ground. |
| Control header | J2 (8-pin), R9 to R18, C19 to C21, D11 to D13 | 1 +3V3_REF, 2 GND, 3 LOAD_EN, 4 GND, 5 SENSE_12V, 6 SENSE_5V, 7 SENSE_3V3, 8 GND. Each sense line has a divider, a filter capacitor and a 3.3 V zener. |
| Indicators | D2 to D8, R19 to R24 | White raw input (through D8), blue +12V_PROT, green +5 V, green +5V_SW, amber +3.3 V, red when F1 has tripped (the voltage across F1 above about 2 V). |
| Test points | TP1 to TP10 | Probe points on the main nodes. |
| Mounting | H1 to H4 | Four M2.5 holes in the corners, at (4, 4), (109, 4), (4, 71) and (109, 71) mm. |

## Power and operation

- The board is powered from J1: 12 V nominal, 12 V +/-5 % (10.8 to 12.6 V).
- The 5 V rail carries up to 1.5 A and the 3.3 V rail up to 0.35 A.
- LOAD_EN on J2 pin 3 switches the +5V_SW output.

## Measured results

KiCad 10.0.6; Synth built from commit `0ceb7b8071d40edadc0fff36584922d6f8c3f917` of `absmach/synth` (`upstream/main`, the merge of Synth pull request #388, which carries #370 and #394 before it); KiCadRoutingTools `9399bfb29e072bfe60b10d9cdae20c274fc2a17a`. The figures below are from the first of the three exports in [`export/`](export/).

| Item | Result |
|---|---|
| Router state | `routed`, `fabrication_ready` True, 0 blocking findings, 0 open nets, 1 non-blocking return-via finding ([`export/protected_power_entry.routing.json`](export/protected_power_entry.routing.json)) |
| Outline | 114.9 x 76.0 mm = 8731 mm2 |
| Parts | 84 (70 excluding the four holes and ten test points); courtyard fill 12.4 % |
| Routing | 34 nets connected, 288 track segments, 44 vias, all 0.6 mm over a 0.2 mm drill (ring 0.20 mm); no track is narrower than its net class, the engine's strict-size verdict is clear; the nearest through-hole pad to an SMD pad is 2.13 mm (`J2.8` to `TP10.1`), above JLCPCB's 2.1 mm |
| KiCad DRC (`--refill-zones --severity-all`, footprint library table configured) | 0 errors, 0 unconnected; 80 `lib_footprint_mismatch` warnings ([`reports/kicad_drc.json`](reports/kicad_drc.json)). Each is a footprint whose silkscreen differs from the library copy (the exporter cuts silkscreen back from vias and pads); its pads, courtyard and fabrication outline are identical to the library file ([`reports/footprint_diff.txt`](reports/footprint_diff.txt), from [`footprint_diff.py`](agent-run/tools/footprint_diff.py)) |
| KiCad ERC (symbol and footprint library tables configured) | 0 errors, 2 warnings ([`reports/kicad_erc.json`](reports/kicad_erc.json)): `Q1` is a P-channel MOSFET whose registry part names the symbol `Si2301CDS`, which KiCad 10.0.6's `Transistor_FET` library does not contain, so the exporter draws its own symbol; and a `BUCK_EN` label sits on a 25.4 mm vertical wire where a second wire of the same net meets it. The tables are [`reports/fp-lib-table`](reports/fp-lib-table) and [`reports/sym-lib-table`](reports/sym-lib-table); [`library_check.sh`](agent-run/tools/library_check.sh) builds them and runs DRC and ERC |
| Gerber and drill content hash | `76a311a42f` (identical across the three exports once the creation time in the Gerber headers is masked) |

Connector distance to the board edge (courtyard to Edge.Cuts, measured on the exported board): J1 on the left edge 0.00 mm; J3, J4 and J5 on the right edge 0.00 mm; the pin header J2 sits 4.27 mm inboard of the bottom edge.

Decoupling capacitors, pad to the pin they serve:

    C1   pad on +12V_PROT -> U1.2    13.8 mm
    C2   pad on +12V_PROT -> U1.2     9.0 mm
    C3   pad on +12V_PROT -> U1.2     5.5 mm
    C4   pad on +12V_PROT -> U1.2     3.6 mm
    C5   pad on BUCK_SW   -> U1.3     3.5 mm
    C6   pad on BUCK_SS   -> U1.8     2.7 mm
    C8   pad on +5V       -> U3.1    13.0 mm
    C9   pad on +5V       -> U3.1     8.3 mm
    C10  pad on +5V       -> U3.1    18.6 mm
    C12  pad on +5V       -> U3.1     5.3 mm
    C13  pad on LS_CT     -> U3.4     5.8 mm
    C14  pad on +5V_SW    -> U3.6     4.6 mm
    C15  pad on +5V       -> U2.3     6.3 mm
    C16  pad on +3V3      -> U2.2     7.5 mm
    C17  pad on +3V3      -> U2.2     9.8 mm
    C18  pad on +3V3      -> U2.2     5.0 mm
    C19  pad on SENSE_12V -> J2.5    10.0 mm
    C20  pad on SENSE_5V  -> J2.6    17.4 mm
    C21  pad on SENSE_3V3 -> J2.7    22.9 mm

Net classes (PWR12 0.6 mm, SWNODE 0.6 mm, PWR5 0.6 mm, PWR3 0.5 mm, PGND 0.8 mm, SIG 0.25 mm, Default 0.2 mm; vias 0.6 mm over a 0.2 mm drill) are declared and assigned by net; every power track of the exported board is at its declared width:

    +12V_PROT 0.6 mm (30 segments), VIN_12V_RAW 0.6 mm (13)
    +5V 0.6 mm (45 segments)
    +3V3 0.5 mm (24), GND 0.2 mm (48 stubs into the two pours) and 2.0 mm (2)

The widths are checked against the current. IPC-2221, 1 oz external copper, 10 C rise: 1.5 A needs 0.53 mm, so the 0.6 mm `+5V` trunk carries 1.5 A, and the 0.6 mm 12 V rails carry the 1.55 A input current of the buck at 10.8 V (15 W / (0.9 x 10.8 V)).

The routing report ([`export/protected_power_entry.routing.json`](export/protected_power_entry.routing.json)) records no engine policy verdict of `rejected`: the routing engine, run with its strict-size policy, delivered no track below its class width, and Synth's independent checks (connectivity, the 0.127 mm process floor, KiCad DRC with the library tables, the return-via rule) pass.

## Placement

The courtyard fill of 12.4 % is tracked in [absmach/synth#427](https://github.com/absmach/synth/issues/427).

The outline, 114.9 x 76.0 mm, is set by the connectors and the corner holes: J1 on the left edge, J3, J4 and J5 flush on the right edge (98 to 106 mm), the control header J2 along the bottom, and a mounting hole in each corner. The functional regions, by the part positions of the exported board:

| Region | Parts | Extent (mm) | Constraint it follows |
|---|---|---|---|
| Input and protection | J1, F1, Q1, D1, D9, R1, C1 | x 7 to 48, y 28 to 41 | Power enters on the left edge; `D1` sits beside `Q1` ahead of it, `F1` 6 mm from `Q1` |
| 5 V buck | U1, L1, C2 to C10, R2 to R6 | x 53 to 76, y 28 to 52 | The switching node is short: `L1` is 7.1 mm from `U1`; the input capacitors `C2` to `C4` and the boot, soft-start and compensation parts sit at the regulator pins |
| Load switch and 3.3 V | U3, U2, C12 to C18, R7, R8, D10 | x 59 to 90, y 44 to 71 | Downstream of the buck; the AMS1117 `U2` is 24 to 32 mm from the buck parts `U1`, `L1` and `U3`, so its 0.60 W is not beside the inductor |
| Outputs | F2, F3, J3, J4, J5 | x 98 to 106, y 18 to 63 | Connector access: all three outputs are flush on the right edge |
| Control header | J2, R9 to R18, C19 to C21, D11 to D13 | x 11 to 54, y 56 to 73 | Reachable from the bottom edge; the through-hole header is at least 2.1 mm from every SMD pad |
| Indicators | D2 to D8, R19 to R24 | x 20 to 99, y 8 to 69 | Each LED sits at the node it shows, along the top and across the board |

Power flows left to right, from `J1` through the protection and the buck to the outputs on the right edge, and the 12 V copper is on the left half. The copper clearance of the power classes is 0.25 mm and the DRC enforces it; the supply is 12 V DC. The two dissipators are the AMS1117 (0.60 W at the supported 0.35 A) and the buck's `U1` and `L1`, which are 24 mm or more apart.

## Input protection

The supply is 12 V nominal, 12 V +/-5 % (10.8 to 12.6 V), and 12.6 V is the qualified maximum. `D1` is an SMAJ13A (Littelfuse SMAJ series datasheet: standoff 13.0 V, 0.4 V (3.2 %) above the maximum input; breakdown 14.40 to 15.90 V at 1 mA, 1.8 V above the maximum input; 1 uA maximum leakage at the standoff, lower at 12.6 V; 21.5 V clamp at 18.6 A) on the fused input, ahead of `Q1`, so a transient is clamped before it reaches the MOSFET. The surge requirement is at most 18.6 A peak (10/1000 us), set by the source impedance. The 21.5 V clamp is 1.5 V below the MP2307's 23 V recommended input, 4.5 V below its 26 V absolute maximum (MPS datasheet) and 8.5 V below the 30 V drain-source rating of `Q1`. A reversed supply forward-biases `D1` and `F1` opens.

Worst case against the datasheet limits, which are minimum and maximum values that already include the part tolerance:

| Condition | Value | Limit | Margin |
|---|---|---|---|
| Supply at +5 % | 12.6 V | `D1` standoff 13.0 V minimum | 0.4 V (3.2 %); leakage at most 1 uA |
| Supply raised to 13.0 V | 13.0 V | `D1` breakdown 14.40 V minimum at 1 mA | 1.4 V (10.8 %) |
| Surge at the rated 18.6 A | 21.5 V clamp | MP2307 23 V recommended input | 1.5 V (6.5 %) |
| Surge at the rated 18.6 A | 21.5 V clamp | MP2307 26 V absolute maximum, `Q1` 30 V | 4.5 V (17 %), 8.5 V (28 %) |

The qualified envelope is a supply of at most 12.6 V and a surge of at most 18.6 A (10/1000 us); a source that can exceed either is outside it. The clamp is lower at smaller surge currents. `D9` holds the gate of `Q1` within 5.1 V of its source, against the 12 V gate-source rating.

## AMS1117 operating envelope

The supported load on the 3.3 V rail is 0.35 A from the 5.0 V rail: P = (5.0 - 3.3) V x 0.35 A = 0.60 W. At the datasheet's 90 C/W for the SOT-223 the junction rises 54 C, so the ambient may reach 71 C at the 125 C junction limit. The load limit at 70 C ambient is (125 - 70) C / (90 C/W x 1.7 V) = 0.36 A.

## Return paths

Ground-return vias at layer changes are tracked in [absmach/synth#280](https://github.com/absmach/synth/issues/280).

The routing report lists 1 non-blocking return-via advisory: a track changes layer at a via with no ground-return via within 1.53 mm and no spot in the GND pour for one. The rule treats a net as fast unless its net class declares `edge_rate slow`; `PWR12`, `PWR5` and `PWR3` declare it, because they are supply rails held by capacitors with no switching edge, while `SWNODE` (the buck's switching node) does not, and nets with analog, clock or differential pins stay critical whatever their class says. It is checked below against the nearest ground via on the exported board. Both layers carry a GND pour, so the return current stays on a continuous plane; the finding is that its path to a stitching via is longer than the rule's window.

| Net | Via position (mm) | Nearest GND via (mm) |
|---|---|---|
| `+12V_PROT` | (60.1, 33.8) | 2.69 |

`+12V_PROT` carries the buck's input current, 1.55 A at 10.8 V, upstream of the input capacitors `C2` to `C4` that supply the switching current. The added loop for the 2.69 mm detour is about 5.4 nH; with a ripple of 20 % of the input current, 0.3 A in 1 us, V = L x di/dt = 5.4 nH x 0.3 A / 1 us = 1.6 mV.

## Routing report and the delivered board

The routing report ([`export/protected_power_entry.routing.json`](export/protected_power_entry.routing.json)) is written when the router finishes. Synth then repairs the candidate before it judges it, and the delivered board ([`export/protected_power_entry.kicad_pcb`](export/protected_power_entry.kicad_pcb)) is the repaired one. The report's segment, via and wire-length counts are not refreshed after the repair passes ([absmach/synth#424](https://github.com/absmach/synth/issues/424)). The counts of the two files, from the same export:

| Count | Routing report | Delivered board | What accounts for the difference |
|---|---|---|---|
| Track segments | 289 | 288 | Synth prunes one dangling stub before the board is judged |
| Vias | 27 | 44 (24 ground, 20 signal and power) | The router's own count, `claimed_total_vias`, is 20 and equals the 20 signal and power vias of the delivered board; Synth adds the further ground vias after the router reports, to tie ground-pour islands and for the return-via repair: 44 - 27 = 17 |
| Connected nets | 34 | 34 | The engine routes 33 single-ended nets (`claimed_successful: 33`, all 33 of 33 in its log) and the `GND` pour completes the 34th |

[`reports/delivered_board_counts.json`](reports/delivered_board_counts.json), made by [`delivered_counts.py`](agent-run/tools/delivered_counts.py) from the delivered board `protected_power_entry.kicad_pcb` (hash `4564963f46`), sets the board's own counts beside the report's: 288 segments, 44 vias (all 0.6 mm over a 0.2 mm drill), 873.4 mm of track, 0 DRC errors and 0 unconnected. The README's measurements are the delivered board's. Open nets: 0, in both files.

## Warnings and their disposition

All figures are for the released artifact, Gerber content hash `76a311a42f`, DRC and ERC with the footprint and symbol library tables configured ([`reports/`](reports/)). Each warning is accepted by JeffMboya's sign-off below, for the reason given.

**DRC: 80 `lib_footprint_mismatch` warnings, 0 errors, 0 unconnected.** Every one is a footprint whose silkscreen differs from its library copy, because Synth cuts silkscreen back from vias and pads so the silkscreen stays 0.25 mm clear of copper ([absmach/synth#425](https://github.com/absmach/synth/issues/425)) (JLCPCB's minimum is 0.15 mm). The pads, courtyard and fabrication outline of each are identical to the library file, checked by [`footprint_diff.py`](agent-run/tools/footprint_diff.py):

| Footprint | Warnings | Differs from the library in |
|---|---|---|
| `Capacitor_SMD:C_0603_1608Metric` | 9 | `F.SilkS`: 9 |
| `Capacitor_SMD:C_1206_3216Metric` | 8 | `F.SilkS`: 8 |
| `Capacitor_THT:CP_Radial_D6.3mm_P2.50mm` | 3 | `F.SilkS`: 3 |
| `Connector_JST:JST_XH_S2B-XH-A-1_1x02_P2.50mm_Horizontal` | 4 | `F.SilkS`: 4 |
| `Connector_PinHeader_2.54mm:PinHeader_1x08_P2.54mm_Vertical` | 1 | `F.SilkS`: 1 |
| `Diode_SMD:D_SMA` | 1 | `F.SilkS`: 1 |
| `Diode_SMD:D_SOD-123` | 6 | `F.SilkS`: 6 |
| `Fuse:Fuse_1812_4532Metric` | 3 | `F.SilkS`: 3 |
| `Inductor_SMD:L_Bourns_SRN6045TA` | 1 | `F.SilkS`: 1 |
| `LED_SMD:LED_0603_1608Metric` | 6 | `F.SilkS`: 6 |
| `Package_SO:SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.3mm` | 1 | `F.SilkS`: 1 |
| `Package_TO_SOT_SMD:SOT-223-3_TabPin2` | 1 | `F.SilkS`: 1 |
| `Package_TO_SOT_SMD:SOT-23` | 1 | `F.SilkS`: 1 |
| `Package_TO_SOT_SMD:SOT-23-6` | 1 | `F.SilkS`: 1 |
| `Resistor_SMD:R_0603_1608Metric` | 24 | `F.SilkS`: 24 |
| `TestPoint:TestPoint_Pad_D1.5mm` | 10 | `F.SilkS`: 10 |

**ERC: 2 warnings, 0 errors.**

| Warning | Why it is acceptable |
|---|---|
| `lib_symbol_issues`: symbol `Si2301CDS` not found in `Transistor_FET` (`Q1`) | The part's registry entry names a symbol that KiCad's library does not contain, so the exporter draws its own symbol ([absmach/synth#96](https://github.com/absmach/synth/issues/96)). Its three pins (gate, source, drain) are connected as the schematic shows and the pin numbers match the SOT-23 footprint. |
| `label_multiple_wires`: the `BUCK_EN` label sits on a 25.4 mm wire where a second wire meets it | Both wires carry `BUCK_EN`, so the label names one net. The netlist is unchanged by it ([absmach/synth#426](https://github.com/absmach/synth/issues/426)). |

## Reproducibility

The source and sidecar in this directory were exported three times through the Synth MCP server (`synth mcp --stdio`, the `synth_export` tool, the call an agent makes), each into an empty directory, router included. The server, the request, the response and the hash of every file written are recorded in [`agent-run/mcp-final-export/`](agent-run/mcp-final-export/). Each cell is the first ten characters of a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `protected_power_entry.synth.kicad_pcb` (un-routed) | df006c77f0 | df006c77f0 | df006c77f0 | byte-identical |
| `protected_power_entry.kicad_pcb` | 4564963f46 | 3efb874c39 | a17d21fa61 | not byte-identical; identical after the router's and KiCad's random ids are masked (normalized) |
| `protected_power_entry.kicad_sch` (one sheet) | e9926cfced | e9926cfced | e9926cfced | byte-identical |
| `protected_power_entry.kicad_pro` | 2e8d84adc8 | 2e8d84adc8 | 2e8d84adc8 | byte-identical |
| `protected_power_entry.kicad_sym` | a908cb0502 | a908cb0502 | a908cb0502 | byte-identical |
| `bom.csv` | 3f8d22508c | 3f8d22508c | 3f8d22508c | byte-identical |
| `pnp.csv` | e00d4fb5ba | e00d4fb5ba | e00d4fb5ba | byte-identical |
| Gerber and drill content | 76a311a42f | 76a311a42f | 76a311a42f | identical after the creation time in the headers is masked (normalized) |

With the random ids masked the routed board is `9ebe15a562` in all three exports ([`reproduce/compare_exports.py`](reproduce/compare_exports.py) does the masking). [`reproduce/reproduce.sh`](reproduce/reproduce.sh) is supplemental tooling, not the evidence of how the board was made (see [`PROVENANCE.md`](PROVENANCE.md)); it rebuilds Synth at the pinned commit, exports three times and compares, then builds the fabrication package.

## Bill of materials and fabrication package

[`fab/`](fab/) holds the files a quote needs: `protected_power_entry_gerbers.zip` (the Gerber layers and the Excellon drill file, from KiCad 10.0.6), `protected_power_entry_cpl.csv` (70 placements in JLCPCB's column order) `protected_power_entry.step` (the board's STEP model, below) and `protected_power_entry_bom_orderable.csv` (the exported BOM joined with [`agent-run/tools/sourcing.csv`](agent-run/tools/sourcing.csv): manufacturer, MPN, package, rating, datasheet or page, distributor and order number for the 84 lines), with `SHA256SUMS` and `GERBER_CONTENT_SHA256`. The STEP model covers every part and mechanical feature of the board. [`step_export.sh`](agent-run/tools/step_export.sh) exports it with KiCad 10.0.6's `kicad-cli pcb export step` from a scratch copy of the exported board that names a model for each footprint: KiCad's stock 3D library has none for `TestPoint_Pad_D1.5mm` (10 test points) or `Fuse_1812_4532Metric` (3 fuses), so [`step_models.py`](agent-run/tools/step_models.py) writes envelope models of them into [`agent-run/step-models/`](agent-run/step-models/) (a 1.5 mm copper disc 0.05 mm proud of the board; a 4.5 x 3.2 mm body 1.25 mm high with 0.6 mm end terminations, the height from the JLCPCB listing of the Littelfuse 1812L150/16DR), and the MP2307 (`U1`) takes the library's `SOIC-8-1EP_3.9x4.9mm_P1.27mm_EP2.41x3.81mm` model, the same body and leads with a 0.51 mm longer exposed pad than the footprint's. The four mounting holes have no body: each is a 2.5 mm hole in the board solid. These are envelope models, not manufacturer CAD.\n\n[`step_verify.py`](agent-run/tools/step_verify.py) reopens the STEP in OpenCASCADE (OCP, `pip install cadquery-ocp`) and compares it with the committed PCB ([`reports/step_verify.txt`](reports/step_verify.txt)): the 80 components of the STEP are the 80 footprints with a body, each under its reference and none repeated; every component origin is the footprint's position (worst difference 0.0000 mm) and every rotation the footprint's (0.000 deg); the board solid is 114.90 x 75.99 mm against the Edge.Cuts extent of 114.90 x 75.99 mm; its holes are the PCB's drills (6 of 0.8 mm, 16 of 1.0 mm and the 4 mounting holes of 2.5 mm; the 44 via drills are not part of KiCad's STEP board); and no body lies more than 0.25 mm outside its courtyard, connectors included. The board solid is 1.51 mm thick; the PCB's general thickness is 1.6 mm. [`step_check.py`](agent-run/tools/step_check.py) ([`reports/step_check.txt`](reports/step_check.txt)) checks the ISO 10303-21 framing and that all 42,980 entities resolve. KiCad's STEP writer numbers its entities differently on every run, so the file hash in `SHA256SUMS` is not reproducible; the geometry hash, the sorted point coordinates rounded to 0.001 mm, is `22f90f3d9f` for all three exports.\n\nRegenerate them with [`fab_package.sh`](agent-run/tools/fab_package.sh).

## JLCPCB

PCB DFM of the package of this README (JLCDFM, 2026-11-04 14:23, Gerber content hash `76a311a42f`, two layers, 11.49 x 7.6 cm, report in [`reports/`](reports/jlcdfm_pcb_dfm_2026-11-04_14-23_76a311a42f.pdf)): 0 Danger items. Trace width 0.25 mm, pad spacing 0.32 mm, annular ring 0.2 mm, silkscreen line width 0.18 mm and silkscreen to pad 0.25 mm are good, with no silkscreen over a hole and no via in a pad.

BOM matching of the package in the JLCPCB assembly flow ([`reports/jlcpcb_assembly_bom_matching_2026-11-04.md`](reports/jlcpcb_assembly_bom_matching_2026-11-04.md), original [`.xlsx`](reports/jlcpcb_assembly_bom_matching_2026-11-04.xlsx)): 66 of 70 lines match a part in JLCPCB stock. The three radial through-hole electrolytics C1, C10 and C17 (NIC NRSZ101M25V6.3X11F) and the 1x8 header J2 are through-hole parts fitted by hand; JLCPCB matched C1, C10 and C17 to an SMD part with 0 in stock and J2 to nothing.

Quote with assembly for the same package ([`reports/jlcpcb_quote_2026-11-04.md`](reports/jlcpcb_quote_2026-11-04.md), Standard PCBA, no order placed): PCB $8.80 and PCBA $130.21, $139.01 in total.

## Human review

Every part of the board is recorded as reviewed by JeffMboya, with the review date and the datasheet or product page, in the 35 part files of [`reproduce/user-registry/`](reproduce/user-registry/) (`reviewed_by`, `reviewed_at`, `datasheet_url`).

### PCB review sign-off

PCB review sign-off required by #39: JeffMboya, 2026-11-04, on the package in `fab/` (Gerber content hash `76a311a42f`). The sign-off is JeffMboya's own. Jeff Mboya is the independent PCB reviewer as well as the author of this pull request. It covers, each verified and approved by JeffMboya:

| Item | Signed off |
|---|---|
| PCB and Gerber package | the Gerbers, drill, BOM and CPL of `fab/` and the exported KiCad board, hash `76a311a42f`, and the STEP model of `fab/` (geometry hash `22f90f3d9f`, complete-model check in `reports/step_verify.txt`) |
| Return paths | the 1 return-via advisory of the routing report, `+12V_PROT` (2.69 mm to the nearest ground via), as checked in "Return paths" above; JeffMboya accepts this arrangement for the buck's switching-current loop: the switching current is supplied by `C2` to `C4` at the regulator pins, and the 2.69 mm detour adds about 5.4 nH, 1.6 mV at the buck's ripple current; and the `edge_rate slow` declaration of the `PWR12`, `PWR5` and `PWR3` net classes |
| ERC | the 2 warnings: `Si2301CDS` absent from KiCad 10.0.6's `Transistor_FET` library (`Q1`) and the `BUCK_EN` label on a wire of its own net |
| DRC | the 80 `lib_footprint_mismatch` warnings, each a footprint whose silkscreen differs from the library copy while its pads, courtyard and fabrication outline match it |
| Power copper | the declared and exported widths: every power track of the export is at its declared class width, `+5V` 0.6 mm against the 0.53 mm that 1.5 A needs, with no narrowed section and no `rejected` engine verdict; the nearest through-hole to SMD pad pair at 2.13 mm |
| Input protection and thermal | the qualified input of 12 V +/-5 % (10.8 to 12.6 V, maximum 12.6 V), the SMAJ13A margins against the MP2307 and `Q1` with its 13.0 V standoff as the upper bound of the checks, and the AMS1117 envelope of 0.35 A to 71 C ambient |
| Placement | the placement of the 84 parts on 114.9 x 76.0 mm, 12.4 % courtyard fill, with the regions, current loops, heat sources and connector access of "Placement" above, connectors flush on the edges, a mounting hole in each corner, and the decoupler distances listed above |
| Parts and values | the BOM identities of `fab/`, including `D9` and `D10` (Micro Commercial Components BZT52C5V1-TP, JLCPCB listing `C78728`), and the values of the circuit |
