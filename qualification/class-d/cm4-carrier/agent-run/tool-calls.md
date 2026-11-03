# TOOL CALLS — CM4 carrier (SYN-407)

Chronological. "MCP" is the Synth MCP server; "CLI" is
`target/debug/synth` built at upstream `a437fac`. Every failed call is
included.

## Context and references

1. `gh issue view 407 --repo absmach/synth` — read the board spec,
   deliverables, acceptance and ground rules.
2. `git fetch upstream` — upstream at `a437fac`.
3. `git worktree add -b syn-407-cm4-carrier ... upstream/main` — clean
   worktree at `a437fac`.
4. Read `qualification/class-b/buck/{README.md,QUALIFICATION.md,buck.synth,
   buck.placement.layout.toml}`, `qualification/class-b/usb-uart/
   {README.md,qualification.json,usb_uart.synth,usb_uart.placement.layout.toml}`,
   `crates/synth-cli/tests/usb_uart_board.rs`, `crates/synth-cli/tests/
   stub_router/mod.rs`, `docs/kicad-workflows.md`.
5. `gh pr view 269 --repo absmach/synth --json files` — not completed; the
   agent-run layout was taken from issue #407's list and the buck README.

## MCP tools

6. `synth_validate` on `buck.synth` — **failed**:
   `E-SYNTH-REGISTRY-001: Component registry not found at path
   '/home/steve/work/synth-mcp-main/registry/parts'`.
7. `synth_validate` with `workspace_root`/`registry_path` — **failed**, same.
8. `synth_search_registry` `df40` — **failed**: could not load registry;
   `mpu6050_imu` declares pin 25, the installed `Sensor_Motion:MPU-6050`
   symbol has no pin 25.
9. Removed the `ep` pin 25 from the MCP checkout's `mpu6050_imu` and from the
   worktree copy (local environment fix, not committed) — search then failed
   on `tmp117_temperature` pin 7, the same class of symbol/pin mismatch. The
   MCP server could not load the full registry.
10. `synth_language_reference` — read the grammar (groups, sheets, netclass,
    diff_pair, stackup, power/net, placement_hint).
11. `synth_search_registry_web` for `DF40C-100DS-0.4V`, `USB2514B-AEZC`,
    `UJ2-ADH-TH`, `JXD6-0001NL`, `47219-2001`, `FH12-22S-0.5SH(55)`,
    `PCF85063ATL`, `DMP3098L-7`, `1058` — ambiguous/no exact match; the
    tool returned no candidate list. LCSC numbers were then found by web
    search.
12. `synth_evaluate_thermal_si` — available; not used for the final geometry
    (single-ended Z0 only, no differential solver).

The MCP server was not usable for this board. All gate commands below are the
CLI equivalents, which run the same compiler/ERC/layout/export code.

## CLI gates and attempts

13. `synth routers` — KRT available with `KICAD_ROUTING_TOOLS_REPO=
    /home/steve/tools/KiCadRoutingTools`; FreeRouting available with
    `SYNTH_FREEROUTING_JAR`/`SYNTH_FREEROUTING_JAVA`.
14. `synth check buck --json` — **fail**: `drc: unknown`; the KRT child
    crashed with `plane pour failed`.
15. `synth drc buck` (KRT) — **validation_failed**: plane pour failed.
16. `synth drc usb_uart` (KRT) — **validation_failed**: fine-pitch escape
    without via-in-pad.
17. `synth validate cm4_carrier.synth` — **timeout** after 120 s, then 300 s,
    then 600 s. The schematic layout does not finish.
18. `synth dump-ir cm4_carrier.synth` — first run exit 1 with
    `E-SYNTH-NAME-005` conflicts and undefined pins; after fixing the nets,
    exit 0 (149 nets, 179 components).
19. `synth place cm4_carrier.synth` — **timeout** at 120 s, then **exit 1**
    with a null placement after 900 s.
20. `synth export-kicad cm4_carrier.synth --out /tmp/opencode/cm4/export1
    --allow-incomplete --router free-routing` — **exit 2**, ERC errors:
    4 `E-SYNTH-POWER-008`, 2 `E-SYNTH-POWER-002`, 13
    `E-SYNTH-CONNECT-001`, 2 `E-SYNTH-POWER-003`, 2 `E-SYNTH-I2C-001`,
    1 `E-SYNTH-KG-001`. Fixed the netlist and part types.
21. Same export, iterations 2–12 — each fixed one class of ERC error
    (POWER-008 supply detection, I2C capabilities, CM4 GND pin types,
    TPS22918 QOD/VOUT, FFC MP pads, RTC CLKOE). The final ERC run reports
    **0 errors** and 100+ warnings.
22. `synth export-kicad ... --out /tmp/opencode/cm4/export13 --allow-incomplete
    --router free-routing` — **timeout at 1800 s**. It wrote the schematic
    (`cm4_carrier.kicad_sch`, four sub-sheets, `.kicad_sym`, `.kicad_pro`,
    `.kicad_dru`) at about 29 minutes and was killed before the PCB.
23. `kicad-cli sch erc --severity-all --format json -o
    reports/cm4_carrier.kicad-erc.json kicad/cm4_carrier.kicad_sch` —
    **93 violations, 13 errors, 80 warnings**. Errors: 5 TPS22918
    QOD/VOUT pin-type conflicts, 1 U4 XTALOUT/PWR_FLAG, 2 FFC MP
    pin-not-connected/power-not-driven pairs, 1 Q_LED gate input not driven,
    1 PWR_FLAG input power not driven. Warnings: 78 `synth` library not
    configured, 1 `Si2301CDS` not found, 1 multiple net names.
24. `synth export-kicad ... --out /tmp/opencode/cm4/export14
    --allow-incomplete --router kicad-routing-tools
    --kicad-routing-tools-repo /home/steve/tools/KiCadRoutingTools` —
    started after fixing the CM4 p95 output type, removing QOD ties and
    tying the FFC MP pads to GND. Result: see `VERIFICATION_REPORT.md`.
25. `cargo test -p synth-cli --test cm4_carrier_board -- --nocapture` — the
    two IR tests pass; the mounting-hole test runs `export-kicad` with the
    stub router and did not finish inside the test budget.
26. `synth check cm4_carrier.synth --json` — **fail**: the `drc` stage calls
    `synth validate`, which times out, so the stage is `unknown`.
27. `synth check cm4_carrier.synth --fab --json` — **fail**: same upstream
    block; no manufacturing stage.

## Datasheets read

28. Raspberry Pi CM4 datasheet — downloaded and read Table 6 (pages 16–22),
    section 5.1 (page 25), Ethernet section 2.2 (page 5).
29. TI LMR33640 SNVSB99C — read pin table, VREF, EN, PG, inductor/cap
    equations.
30. TI LMR38010 SNVSBM3B — read pin table and the buck-board values.
31. Microchip USB2514B DS00001692C — read the 36-pin pin list, CFG_SEL table
    5-1, crystal section 7.1, strapping.
32. NXP PCF85063A Rev. 7.3 — read the DFN2626-10 pinout, I2C, supply range,
    CLKOE.
33. Pulse JXD6-0001NL product page — 1000BASE-T, 1:1, 10 pins + shield.
34. Same Sky UJ2-ADH-TH 1.04 — dual stacked USB-A, 8 contacts + shield.
35. Molex 47219-2001 SD-47219-001 — 8 contacts, hinge, no card detect.
36. Hirose FH12-22S-0.5SH(55) product page — 22 positions, 0.5 mm, 0.5 A.
37. Diodes DMP3098L Rev. 08-1 — −30 V, ±20 V Vgs, −3.8 A.
38. Keystone 1058 catalogue page — CR2032/CR2025 SMD holder.
39. KDS DST310S LCSC page — 32.768 kHz, 12.5 pF.
40. PTTC SMD1812P260TF/16 LCSC page — 2.6 A hold, 16 V.

## Part authoring

41. Wrote `registry/parts/connectors/hirose_df40c_100ds_0v4_51.synth.toml`
    and `..._b.synth.toml` (100 pins each, CM4 Table 6 types, synthesized
    symbols, `reviewed_by = ""`).
42. Wrote `registry/parts/ic/usb2514b.synth.toml` (37 pins, QFN-36,
    `reviewed_by = ""`).
43. Wrote `registry/parts/connectors/usb_a_stacked_cui_uj2_adh_th.synth.toml`,
    `rj45_pulse_jxd6_0001nl.synth.toml`,
    `microsd_molex_47219_2001.synth.toml`,
    `ffc_22p_hirose_fh12_22s_0_5sh.synth.toml`,
    `mechanical/coin_cell_keystone_1058.synth.toml`,
    `ic/pcf85063atl.synth.toml`,
    `crystals/xtal_32k768_3215.synth.toml`,
    `crystals/xtal_24mhz_3225.synth.toml`,
    `transistors/p_mosfet_dmp3098l.synth.toml`,
    `protection/polyfuse_2r6a_16v.synth.toml`.
44. `synth registry qualify --part <id>` on each — all pass pin/pad coverage;
    the only blocking finding is `E-SYNTH-QUAL-008` (no reviewer), which is
    required to stay empty. Two 3D-model warnings and the FFC/RTC exposed-pad
    findings were fixed by adding the `MP`/`11` pins.
45. Extended `registry/parts/ic/txs0108e.synth.toml` with I2C capabilities on
    a1/a2/b1/b2, and `registry/parts/connectors/hirose...` with I2C
    capabilities on the CM4 SCL/SDA pins, for the I2C ERC.
46. Removed the `ep` pin 25 from the worktree `mpu6050_imu` part as a local
    environment fix so the registry loads; **not committed**.

## Tests and artifacts

47. Wrote `crates/synth-cli/tests/cm4_carrier_board.rs` (pin map, GPIO header,
    mounting holes). `cargo test --no-run` compiles.
48. Wrote `qualification/class-d/cm4-carrier/cm4_carrier.placement.layout.toml`
    with all 185 components pinned.
49. Wrote `README.md`, `QUALIFICATION.md`, `qualification.json`,
    `BOM_sourcing.md`, `agent-run/*`.
50. `kicad-cli sch erc` on the first exported schematic — 13 errors, see 23.
