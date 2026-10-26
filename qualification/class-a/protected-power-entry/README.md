# Protected Power Hub

A 12 V protected power-entry and distribution board: a fused, reverse-polarity
protected input with a TVS clamp, an MP2307 buck to 5 V, a TPS22918 switched
5 V output, an AMS1117 3.3 V rail, a control header and six status LEDs. Two
layers, JLCPCB, 110.5 × 72.5 mm, 84 parts, 34 nets.

**Status: an engineer-review draft, not a release.** The board is routed and
KiCad reports nothing wrong with it, but it has no Gerbers, part limits were
written from memory, and the open items below are real. Nothing here was built
or tested on hardware.

**Stated exception.** Every other Class A board has an MCU, USB-C programming
and SWD. This one has none of them: it is a power stage, and nothing on it is
programmable. Its control header takes an enable and gives back three scaled
rail samples, for whatever board plugs into it.

## How this board was made

An agent drove `synth mcp` from `agent-run/prompt.md`, with only the Synth MCP
and Read/Write/Edit, from an empty workspace and an empty parts registry. Its
tool calls are in `agent-run/tool-calls.md` (125 calls, 14:52 to 16:36, about
1 h 43 min) and its own report is `agent-run/report.md`. No file in this
directory was edited by hand after the run.

The five registry parts the agent wrote are their own pull request, #324. The
board also needs `polyfuse_1r5a` and `smaj15a_tvs` from #266, which main does
not have yet.

## What was measured

Measured on the exported files, not the agent's report.

| Check | Result |
|---|---|
| `kicad-cli pcb drc --refill-zones --severity-all` | 0 violations, 0 unconnected items, 0 schematic-parity errors |
| Nets closed | 34 of 34, 0 open |
| Routed copper | 426 segments; 43 vias in the delivered board |
| Board outline | 110.5 × 72.5 mm |
| Schematic | one A1 sheet, rendered with `kicad-cli sch export pdf` |
| Router verdict | `fabrication_ready: true`, 0 blocking reasons |

Circuit checks read from the exported netlist:

- Q1 (Si2301) has its drain on the fused input and its source on `+12V_PROT`,
  so a reversed supply is blocked by the body diode. Its gate is held to 5.1 V
  below the source by D9 (BZT52C5V1) with a 47 k pull.
- D1 (SMAJ15A) sits on `+12V_PROT`, after Q1, so a reversed supply never
  reaches it.
- The MP2307 feedback divider is 44.2 k over 10 k: 0.925 V × (1 + 4.42) ≈ 5.0 V.

## Limitations

- **Vias.** All 43 vias are 0.6/0.3 mm. The `PWR12` and `PWR5` classes ask for
  0.8/0.4 mm, so the two vias on `+12V_PROT` carry the input current on a
  smaller barrel than declared.
- **Narrowed copper.** The router narrows power nets at pad entries, as low as
  0.4 mm on `+12V_PROT` and `VIN_12V_RAW` and 0.2 mm on `+5V`; GND is mostly
  0.2 mm stubs into the two ground pours.
- **Load.** 5 V up to 1.5 A and 3.3 V up to 0.35 A, below the 2.0 A and 0.5 A
  targets. F1 limits the first and the AMS1117's copper the second.
- **Part limits are from memory.** The agent could not open a datasheet. The
  TPS22918 pin map and the MP2307 compensation are unverified.
- **Fault output.** Nothing on the board produces a fault or power-good signal,
  so the header has none. F1 tripping lights a red LED.
- **Silkscreen.** Synth cannot place text, so the board has no legends.
- **Footprint.** The registry MP2307 footprint names a KiCad footprint that
  does not exist; the agent chose `EP2.41x3.3mm` for the new part.
- **BOM.** Several parts carry no LCSC number.

## Reproducibility

The source in this directory was exported three more times with
`synth export-kicad`, router included, and compared with the files the agent's
own `synth_export` wrote. Each cell is the first ten characters of a SHA-256.

| file | agent (MCP) | export 1 | export 2 | export 3 | |
|---|---|---|---|---|---|
| `protected_power_entry.kicad_sch` | 168b27c96a | 168b27c96a | 168b27c96a | 168b27c96a | identical |
| `protected_power_entry.kicad_pro` | ef134432e9 | ef134432e9 | ef134432e9 | ef134432e9 | identical |
| `protected_power_entry.kicad_sym` | 9258824cb9 | 9258824cb9 | 9258824cb9 | 9258824cb9 | identical |
| `bom.csv` | b5eb123bf5 | b5eb123bf5 | b5eb123bf5 | b5eb123bf5 | identical |
| `pnp.csv` | cafdcdc7ce | cafdcdc7ce | cafdcdc7ce | cafdcdc7ce | identical |
| `protected_power_entry.kicad_pcb` | 02d5529935 | e8859f6285 | 7b905a7a01 | 44803d5838 | identical after zone-fill ids |

The four PCB files differ in 458 lines, every one a `uuid` line, the id KiCad
gives each filled-zone polygon when it refills a pour. With those masked, the
four files hash the same (5d388a1913), so the copper is the same. That is four
exports from one source, on one machine, with one router build; it is not a
claim about other machines or router versions. Each export took 21 to 24
minutes.

```
synth export-kicad protected_power_entry.synth --out <dir> --router kicad-routing-tools
```

## Files

- `protected_power_entry.synth`: the board.
- `protected_power_entry.placement.layout.toml`: the pinned placement.
- `agent-run/`: the prompt, the agent's report, its tool calls and its design
  notes.
