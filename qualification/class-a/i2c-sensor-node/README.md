# I2C sensor node

A field sensor module: a BME680 and a 24LC256 on one I2C bus, an STM32G030
reading them, a 12-pin GPIO header, a UART break-out, and a USB-C port that
powers and programs the board through a CH340C bridge. Two layers, JLCPCB.

**Status: an engineer-review draft, not a release.** The board is routed and
KiCad reports no error on it, but every part the agent wrote is unreviewed, the
BOM is not orderable, and the open items below are real. Nothing here was built
or tested on hardware.

## How this board was made

Nobody edited the design. An agent produced it from one prompt, driving
`synth mcp`, and the run was logged.

| | |
|---|---|
| Prompt | [`agent-run/prompt.md`](agent-run/prompt.md), the Class-A specification with this board's emphasis |
| Model | `claude-sonnet-5-5` |
| Tools | the Synth MCP server, plus Read, Write and Edit; no shell, no web |
| Start | an empty workspace and an empty user parts registry |
| Run | 132 tool calls over 133 turns in 64 minutes 28 seconds; about $5.00 |
| Calls | [`agent-run/tool-calls.md`](agent-run/tool-calls.md) lists every one |
| Its documents | the design notes, pin map and power budget it wrote, in `agent-run/` |

The run ended because the agent reached its own usage limit, so it wrote no final
report, and its last `synth_export` did not return a release. The board here is
its final source, `i2c_sensor_node.synth` and its placement sidecar, exported with
`synth export-kicad`, whose output matches the agent's own export byte for byte
(see the irrigation controller's README for that comparison). The six registry
parts it authored or imported are in `registry/parts/`.

Earlier runs of the same prompt exposed defects in Synth, and each was fixed in
Synth rather than in the board: #283, #284, #286, #288, #289, #291, #293, #295,
#297, #299, #301, #303, #308, #310, #312, #314 and #316. This board needs them.

## What was measured

All figures come from the exported files, checked with `kicad-cli` 10.0.6.

| | |
|---|---|
| Board | 92.55 x 62.54 mm, two layers, 87 components, 51 nets |
| Routing | state `routed`, 51 of 51 nets connected, 984 segments, 188 vias |
| KiCad DRC, `--refill-zones` | 0 errors, 0 unconnected, 2 warnings (one dangling track, one silkscreen over copper) |
| Release gate | `release_ready` true; every part still reads `reviewed: false` |
| Schematic | A1, 9 labelled regions that do not overlap |
| Trace width, as routed | `+5V_PROT`, `VIN_5V_RAW` and `VIN_5V_FUSED` 0.5 mm; `+3V3` 0.2 to 0.4 mm |
| Vias | 166 at 0.6 mm, 20 at 0.8 mm, 2 at 0.56 mm; 91 are on GND |
| Return paths | 17 of 97 signal vias have no ground via within 1.53 mm |
| Reset button | 8.9 mm from the MCU |
| BOM | 87 rows; 14 have an LCSC number and 29 a manufacturer number |

The schematic was rendered from the exported `.kicad_sch` with
`kicad-cli sch export pdf`, not from the agent's preview.

## The review of this board

| Review point | State |
|---|---|
| Default export valid | The export above is the default one, with no optional flag, and it passes. |
| Three identical exports | See below. |
| Exposed I/O protection | Each of the 12 GPIO lines has 330 ohm in series and a PRTR5V0U2X ESD array; the UART lines and the USB data and configuration lines have arrays too. Level shifting is not added: the header is 3.3 V and says so. |
| Net geometry | Classes are declared and applied: the 5 V path routes at 0.5 mm. |
| Regulator thermals | The agent budgets 0.18 W in the AMS1117 at 0.1 A. Synth's own estimate assumes 0.8 A and reads 107 C. Neither is a measurement. |
| BOM identity | Not resolved: 14 of 87 rows have an LCSC number. |
| Clamp and part review | Not resolved: the input clamp and every authored part are unverified. |

## Reproducibility

The source in this directory was exported three times with `synth export-kicad`,
router included, and the files compared. Each cell is the first ten characters of
a SHA-256.

| file | export 1 | export 2 | export 3 | |
|---|---|---|---|---|
| `i2c_sensor_node.kicad_sch` | 64ed7d7c38 | 64ed7d7c38 | 64ed7d7c38 | identical |
| `i2c_sensor_node.kicad_pro` | cfe0a30e58 | cfe0a30e58 | cfe0a30e58 | identical |
| `i2c_sensor_node.kicad_sym` | e95b9b9b04 | e95b9b9b04 | e95b9b9b04 | identical |
| `bom.csv` | 969af47848 | 969af47848 | 969af47848 | identical |
| `pnp.csv` | 7ffa5fb360 | 7ffa5fb360 | 7ffa5fb360 | identical |
| `i2c_sensor_node.kicad_pcb` | b1caeb8610 | d4728dfb58 | 514975b04e | identical after zone-fill ids |

The three PCB files differ only in 1,096 `uuid` lines, the ids KiCad gives each
filled-zone polygon when it refills a pour. With those masked, the three files
hash the same, so the copper is the same. That is three exports from one source,
on one machine, with one router build; it is not a claim about other machines or
router versions. Each export took about five minutes.

```
synth export-kicad i2c_sensor_node.synth --out <dir> --router kicad-routing-tools
```

## Open items

- **Return paths:** 17 layer changes have no ground via within 1.53 mm. The pass
  that adds them keeps clear of every pad, which leaves some without a spot.
- **Part review:** the BME680, the CH340C, the STM32G030 variant, the EEPROM and
  both headers were written by the agent from memory of their datasheets
  (its documents say "recalled") and are unreviewed.
- **Regulator:** the AMS1117 dissipation and the input clamp are estimates.
- **Silkscreen:** `.synth` has no way to put text on the board, so pin-1 marks and
  legends are not there. One silkscreen-over-copper warning remains.
- **No Gerbers.** `synth_export` does not write them; `synth export-kicad
  --gerbers --drill` does, and it refuses parts nobody has reviewed.
- **Zone fill:** refill in KiCad (B) before judging the pours; the DRC above
  refilled them itself.

## Files

| File | |
|---|---|
| `i2c_sensor_node.synth` | the design, as the agent wrote it |
| `i2c_sensor_node.placement.layout.toml` | its placement sidecar, in board millimetres |
| `agent-run/` | the prompt, every tool call, and the agent's design notes, pin map and power budget |
| `registry/parts/` | the six parts the agent authored or imported |

One part, `polyfuse_500ma`, comes from #266.
