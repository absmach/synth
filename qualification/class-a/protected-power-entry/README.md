# Protected power entry

A power front end and distribution module on its own board: 12 V in,
reverse-polarity P-FET, resettable over-current, TVS clamp, a buck down to
5 V, a soft-start load switch with its enable brought out, and a housekeeping
3V3 rail. No MCU — the protection here is analogue and wants none.

It needs nothing but a 12 V supply to come up, and a downstream system needs
nothing of its own to use the protected output.

Design rationale is in the source comments next to the circuits it explains;
this file is the specification.

## Board

| | |
| --- | --- |
| Size | 59.52 × 53.50 mm |
| Layers | 2 |
| Process | JLCPCB standard (0.127 mm track/clearance, 0.3 mm drill, 0.13 mm ring) |
| Components | 42 |
| Nets | 18 |
| Source | `protected_power_entry.synth` |

## The protection chain, in fault order

| Stage | Part | Against |
| --- | --- | --- |
| 1 | Q1 Si2301 P-FET, R1 100k, D1 12 V zener | reverse polarity — the FET blocks before anything downstream sees it |
| 2 | F1 polyfuse, 1.5 A | over-current, resettable |
| 3 | D2 TVS array, SMAJ15A | transients, clamped after the fuse so the fuse limits what the TVS conducts |

A TVS in front of the fuse has nothing to limit the fault current it conducts,
which is why the order is the order.

## Rails

Four, declared in the source rather than described in a comment, so the
regulator-range and load-budget rules have something to check and the exported
schematic names them instead of numbering them.

| Rail | Nominal | Source | Available | Feeds |
| --- | --- | --- | --- | --- |
| `V12_PROT` | 12 V | J1, through the chain above | 1.5 A, set by F1 | the buck, D4 |
| `V5` | 5 V | U1 MP2307, L1 6.8 µH, 10k/3.16k divider | 800 mA declared | the load switch |
| `V5_SW` | 5 V | U2 TPS22918, R4 100k, C5 10 nF soft-start | 800 mA declared | J2, U3 |
| `V3V3` | 3.3 V | U3 LP5907 | 250 mA declared | D3, and a carrier's logic |

U1's exposed pad is tied to GND. It is the buck's heat path and the part
declares it `required`, so leaving it floating is an error rather than a
silent thermal fault.

R6/C8 on COMP is the buck's loop compensation, and it is not optional: COMP is
the error amplifier's output, and left floating the loop has no dominant pole,
so the converter rings or refuses to regulate. The values are the datasheet's
typical network for a 5 V output, not a measured result — a reviewer with a
load step should check them. C9 on SS sets the start ramp.

An LP5907 carries the 3V3 rail rather than an AMS1117. This rail drives an
indicator and whatever logic a carrier adds, so it needs tens of milliamps,
not 800 — and an AMS1117 declared at its full rating puts its own junction
near 107 °C through 60 °C/W.

## Connectors

| Ref | Part | Edge | Carries |
| --- | --- | --- | --- |
| J1 | 2-pin JST XH | left | 12 V in, GND |
| J2 | 1×4 header, 2.54 mm | right | switched output and its control |

Power in one side and out the other, which is how a module like this gets
wired into a system and also what keeps both connectors clear of the corner
mounting holes.

**J2 pinout**

| Pin | Signal | Direction |
| --- | --- | --- |
| 1 | `V5_SW`, switched 5 V | out |
| 2 | GND | — |
| 3 | enable, active high, pulled up on board through R4 100k | in |
| 4 | GND | — |

The enable sits between two grounds, so a short to either neighbour turns the
output off rather than on. A host only has to pull pin 3 down to shed the
load; left open, the rail comes up by itself.

R7 on the TPS22918's QOD discharges the output when the switch opens. Without
it a capacitive load holds the rail up after shutdown and the next soft-start
begins from an unknown voltage.

## Indicators and test points

| Ref | Reports |
| --- | --- |
| D4, red, R8 12k | `V12_PROT` present — behind the FET and the fuse, so it is dark on reverse polarity and dark on a tripped fuse |
| D3, green, R5 1k | `V3V3` present, which means the whole chain came up |

| Ref | Probes |
| --- | --- |
| TP1 | `V12_PROT` |
| TP2 | `V5` |
| TP3 | `V5_SW` |
| TP4 | `V3V3` |
| TP5 | GND, the reference for the other four |

## Mechanical

Four M2.5 non-plated holes, hinted to the four corners, and two fiducials.
The corner hints are not decoration: all three mechanical footprints draw
their courtyard as a single `fp_circle`, and until their registry entries
carried the right margin they reserved less area than KiCad checks them
against — see *Limitations*.

## Measured

One local run of the export and the external router.

| | |
| --- | --- |
| Nets closed | **18 of 18** |
| Open | **0** |
| Routed copper | 582.3 mm over 179 segments and 32 vias |
| Tracks below the 0.127 mm floor | **0** |
| Vias below the 0.3 mm drill or 0.13 mm ring | **0** |
| Vias inside pads | **1** — a stitching via in `U2.2`, the load switch's ground pad |
| `kicad-cli pcb drc` | **0 errors**, 0 unconnected items, 0 warnings |
| Routing verdict | `validation_failed` — not fabrication-ready |

Nothing is left for KiCad to report at any severity it checks: no violation,
no unconnected pad, no schematic-parity difference. Every net closes.

The package is still refused, and for one reason: a single stitching via lands
inside `U2.2`, and this board declares the standard tier, which does not fill
vias. The via is on the same net as the pad, so it shorts nothing — it is the
ground pour reaching the bottom layer under the load switch — but an unfilled
via in a pad wicks solder during assembly, so refusing it is right. The board
does not declare `requires via_in_pad`, because asking a two-layer power
front end to be built on a filled-via process for one ground via is a real
cost increase for no electrical gain. A reviewer who wants this board
fabricable has two honest options: declare the filled-via tier, or move the
pour's connection off that pad by hand.

The four `courtyards_overlap` errors this board reported before were the
mechanical parts reserving their feature size instead of their courtyard, and
they are gone with #273's fix.

## Fabrication

No fabrication artifacts, and that is the correct outcome: the routed board
fails independent validation on the via above, so the package is refused as
`UNTRUSTED / NOT FOR FABRICATION` rather than exported. `bom.csv` and the
KiCad project are written either way; Gerbers, drill files and the position
file are not.

## System boundary

What this board is, stated so an integrator does not have to read the source:

1. **Accepts** 12 V nominal on J1, 2-pin JST XH, polarity-protected.
2. **Generates** `V5` always-on, `V5_SW` switched, `V3V3` housekeeping.
3. **Supports** 1.5 A at the input, set by F1; 800 mA declared on each 5 V
   rail and 250 mA on 3V3. The 5 V figures are the regulators' declared
   maxima, not a thermal measurement on this copper.
4. **Exposes** the switched 5 V rail, its return and its enable on J2, and
   five probe points.
5. **Provides** test access on every rail plus a ground reference; there is no
   programming interface because there is nothing programmable.
6. **Still needs**, externally: a 12 V supply and whatever it powers. Nothing
   else.
7. **Limits**: 12 V in, 5 V out, 1.5 A, 2.54 mm header pitch on the output.
   Not fabricable as it stands — one stitching via in a pad against a process
   that does not fill them. No thermal measurement, no conducted-emissions
   figure, no reviewer sign-off.

## Limitations

- **One stitching via lands in `U2.2`** and the declared process does not fill
  vias, so the package is refused. Same-net, so it shorts nothing; see
  *Measured* for why the tier is not simply changed.
- **Four parts are operated outside the envelope their registry entries
  declare.** `jst_xh_2pin`, `p_mosfet_si2301`, `polyfuse` and `d_zener_5v1`
  each declare 5 V and 100 mA, which is a template default rather than a
  datasheet figure — the real devices are a 250 V 3 A connector, a −20 V
  −3.1 A FET, a PPTC and a zener. `header_1x4` declares no envelope at all.
  Filed as #275. Until it is corrected, this board's current capability cannot
  be checked by anything but a reader.
- **`d_zener_5v1` with `value "12V"`** is the clearest case: the board wants a
  12 V zener to clamp Vgs and the registry has only a 5V1, so either the part
  is wrong for the job or a `d_zener_12v` is missing. Also #275.
- The divider ratio, the compensation network and the soft-start values are
  datasheet and convention, not measurement. Nothing checks what the copper
  around the buck does with the heat it dissipates.
- The mechanical parts' courtyards were corrected in this stack's registry
  change; the narrower half of #273 — `courtyard_rect` ignoring `fp_circle`
  entirely — is still open, so a part whose declared body is not its courtyard
  will not be caught automatically.
- Twenty of its parts carry no distributor identity, so the BOM is not
  orderable as it stands.
- Three `E-SYNTH-SCHEM-001` and two `E-SYNTH-SCHEM-003` findings are
  schematic-readability, not electrical: ground flags pointing the wrong way
  and a decoupling capacitor drawn far from the part it serves.
- No independent reviewer has signed off.
