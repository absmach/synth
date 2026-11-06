# REPORT_FIX

## Status in one paragraph

F1 to F5 and the grouping part of F8 are done in the `.synth` and the sidecar. **The board does not release.** The best export (`export_F6d/`) routes 43 of 43 nets with 0 KiCad DRC errors and 0 unconnected, but the release gate fails (`fabrication_ready` false). F6 was only partly done, F7 and F9 were not done, and no final `export/` folder was produced because no state released.

The unchanged board did **not** release either, contrary to the brief. `export_start/` was `validation_failed` with 3 vias in pads (`C_N11.2`, `C_N1.2`, `C_N3.2`), a floating GND island on F.Cu near (53.79, 27.80), 12 signal vias without a ground return via, and an unescapable 0.5 mm DFN (`U3`). KiCad DRC there: 0 errors, 1 unconnected, 112 warnings. Every failure after my changes is the same class of router/gate finding, so I could not tell a regression from the baseline condition.

Not measured: I had no shell, so there were no `kicad-cli pcb drc` runs of my own, no schematic PDF render, and no connector-gap or distance measurement from the exported files. Every number below is from the `synth_export` result or the source.

## Exports

| Folder | What | nets routed | KiCad DRC err / unconn / warn | Blocking reasons |
|---|---|---|---|---|
| `export_start` | unchanged board | 43/43 | 0 / 1 / 112 | 3 via-in-pad, 1 GND island, 12 return-via, DFN escape |
| `export_F1_F5` | F1 to F5 and F8 grouping, F3 removal | 43/43 | 0 / 0 / 110 | 1 via-in-pad (`D_N2.1`), 8 return-via |
| `export_F6a` | + `C_U4`, `C_U5`, `SW_RESET` moved | 43/43 | 0 / 0 / 108 | 1 via-in-pad (`D_N2.1`), 6 return-via |
| `export_F6b` | + `D_N2` moved apart from `R_N4` | 43/43 | 0 / 0 / 108 | 1 via-in-pad (`R_N5.2`), 6 return-via |
| `export_F6c` | `R_N5` to (53.5, 61) | 43/43 | 0 / 0 / 108 | 2 via-in-pad, 6 return-via (worse, dropped) |
| `export_F6d` | `R_N5` to (48, 60.5) | 43/43 | 0 / 0 / 108 | 1 via-in-pad (`R_D1.2`), 6 return-via |

The current sidecar is the F6d state. Nothing passed `allow_incomplete`. I did not call `synth_compact_layout`.

## F1 Protection ratings (done in source, not released)

- `D_N4`, the `d_tvs_5v` across the regulated +3V3 rail, is removed. Nothing replaces it.
- `D_FZ` (was `d_zener_5v1`) is now `d_zener_3v3`, cathode on `FLOW_PULSE`, anode on GND, behind `R_FS` 2.2 k.
- `D_N3`, the 5 V TVS on `FLOW_PULSE`, is removed. With `D_FZ` now a 3.3 V clamp on the same node, a second device would only duplicate it. I did not replace it with a second zener in parallel; say so if the brief meant that literally.
- Current: the sensor output is at most the 13.2 V supply. (13.2 - 3.3) / 2.2 k = 4.5 mA (12 V: 4.0 mA). Zener power is about 3.3 V x 4.5 mA = 15 mW. The rating of the registry part `d_zener_3v3` is not in the registry: **needs sourcing** (a BZT52C3V3-class part is nominally 500 mW; not checked against a datasheet).
- The other clamps in the source: `D_QG` (5.1 V zener, Q1 source-gate clamp; Si2301 gate limit is about 8 V per my recollection, **needs sourcing**) and `D_TVS` (see F2). A 5 V clamp remains on no 3.3 V net.

## F2 Input TVS (done in source)

- `D_TVS` is now `smaj13a_tvs`, authored in `user-registry/smaj13a_tvs.synth.toml`: Littelfuse SMAJ13A, SMA footprint `Diode_SMD:D_SMA` (same as the old part), 13 V standoff, 14.4 to 15.9 V breakdown, 21.5 V clamp at 18.6 A.
- **The datasheet URL in its provenance is a Littelfuse product-series page that I composed from the site's usual pattern; I could not fetch it. Verify it.** The electrical values are those given in the brief.
- Margin: the supply maximum is 13.2 V, 0.2 V over the 13.0 V standoff and 1.2 V under the 14.4 V minimum breakdown. Leakage at 13.2 V is above the standoff figure but below breakdown; its size **needs sourcing**.
- Worst-case clamp 21.5 V < 23 V buck maximum input, margin 1.5 V.
- Not resolved: `Q1` (Si2301, I recall a -20 V drain-source rating) sits upstream of the TVS, so a surge there is not clamped by `D_TVS`. Check against the Si2301 datasheet.

## F3 SHT30 (done)

`U3` and `C_U3` are removed from the source, all nets and the sidecar. I2C pull-ups and `J_I2C` stay. The notes and `README.md` say an SHT30 breakout attaches to `J_I2C` at 0x44, which stays free. The DFN escape finding is gone from `export_F1_F5`.

## F4 Crystal (done)

Crystal NDK NX3225GD-8MHZ-STD-CRA-3, CL 8 pF. CL = C1 x C2 / (C1 + C2) + Cstray, Cstray 5 pF (assumed, not measured), so C1 = C2 = 2 x (8 - 5) = 6 pF. Nearest E24 value 6.2 pF: CL = 3.1 + 5 = 8.1 pF, within 0.5 pF. `C_X1` and `C_X2` are 6.2 pF. They stay at their sidecar positions beside the clock pins.

## F5 Valve current (stated, partly checked)

- Stated in the source header and in the notes of both valve groups: 12 V DC solenoid, at most 0.5 A. I did not find a current attribute for nets in the language, so there is none.
- Tracks: `VALVE1_SW`, `VALVE2_SW` and `+12V_VALVE` are already in netclass `PWR12` (0.8 mm). By my IPC-2221 estimate, 0.5 A at a 10 C rise needs about 0.1 mm of 1 oz outer copper. This is a calculation, not a measurement from the exported file.
- MOSFET: AO3400A, a 30 V part; its drain-current rating is not in the registry, **needs sourcing** (I expect several amps).
- Flyback diode: registry `d_schottky` lists MPN 1N5819 (1 A class) but footprint `Diode_SMD:D_SOD-123`. A 1N5819 is a through-hole DO-41 part; the SOD-123 variant has a different MPN. **The MPN and footprint disagree in the registry and need fixing**; I did not change the part.

## F6 Placement distances (partly done)

Done (sidecar only): `C_U4` to (55, 44.5), `C_U5` to (54, 56), `SW_RESET` to (43, 40.5), `D_N2` to (61.5, 63.5), `R_N5` to (48, 60.5). These were chosen by hand from sidecar coordinates without seeing free space; I did not measure the resulting distances on the exported board. The placement review reported before: reset-related `R_NRST` 21 mm, `C_U4` 11 mm, `C_U5` 17 mm, `TP_NRST` 8 mm. Note the review sized the board at 151 x 113 mm and reported all connectors "inside the edge" by 2 to 38 mm, which I took as an artifact of that sizing, since the board outline is 75.1 x 65.4 mm.

Not done: the debug header (`J_SWD`, 43.8 mm from the processor), the pull-ups `R_SDA`/`R_SCL`, `C_IN1`/`C_IN2`, the rail capacitors `C_N1`, `C_N2`, `C_N10`, `C_N11`, `C_N12`, `C_OUT2`, `TP_NRST`, and the identical-circuit and row-alignment items. The previous README records that moving the reset parts broke routing; I did not retry them. No distances were measured from the exported files.

## F7 Return paths (not done)

`export_F6d` still lists 6 signal vias without a ground return via within 1.53 mm: at (36.40, 20.70), (39.00, 52.30), (30.40, 38.30), (27.00, 52.30), (52.50, 28.50), (27.10, 16.40). Each message says no spot in the GND pour holds a 0.6 mm via clear of the pads, vias and tracks. I found no Synth tool that places a ground via by hand, and moving the layer change needs routing control I do not have here.

## F8 Schematic and silkscreen (grouping done, rest not)

Every previously ungrouped part is now in a named group: `3V3 RAIL DECOUPLING`, `12V RAIL DECOUPLING`, `RAIL INDICATORS`, `RAIL TEST POINTS`, `MOUNTING`. Validation: 0 errors, 52 warnings. I did not render the schematic or review silkscreen findings.

## F9 Bill of materials (not done)

No part was given a manufacturer, MPN or LCSC number. `synth_validate` shows `W-SYNTH-SUPPLY-002` (no `mpn` or `lcsc_pn`) for connectors, `Q1`, `D_QG`, `C_BULK`, test points, `U2`, `SW_RESET`, `J_SWD`, `U4`, `U5`, the LEDs, `D_FZ` and the mounting holes. Generic passives are not specified either. All of that **needs sourcing**. `smaj13a_tvs` also shows `W-SYNTH-PART-UNVERIFIED` until a reviewer is recorded.

## Final figures

- release_ready / fabrication_ready: **false** (best export `export_F6d`)
- route_complete: true, 43 of 43 nets
- KiCad DRC (from the export result): 0 errors, 0 unconnected, 108 warnings; I did not read the individual warnings
- Outline, connector gaps, part count: not measured. The outline is fixed by the sidecar corners and should be unchanged at 75.1 x 65.4 mm; the part count should be 112 - 4 = 108 (U3, C_U3, D_N3, D_N4 removed).

## Why it is not released, and what I would do next

The remaining blockers are router/gate findings, not DRC errors: one ground via lands inside a pad in the crowded bottom-right area (it moved from `D_N2` to `R_N5` to `R_D1` as I changed placement), and 6 return-via advisories. Next steps are an agent run with a router that places ground vias, or opening space near the bottom connectors so the pours have room. I did not find a way to do either with the tools in this session.
