# Final report: SYN-408 BLDC stack

## Branch and commits

Branch `syn-408-bldc-stack`, based on `upstream/main` at `7cadbb59`
(2026-11-03). Commits:

- `dcaf8d8f` feat(ir): check mapped pins, coverage and voltage domains across boards
- `1051b317` feat(registry): author bldc power-stage, connector and op-amp parts
- `d040ec15` feat(qualification): add the class-D BLDC stack
- one final commit: `fix(qualification): use repo-relative paths in the BLDC reports`

The worktree is `/home/steve/work/synth-408`; nothing was pushed and no PR was
opened.

## Acceptance checklist (from issue #408)

| Item | Verdict | Proof |
| --- | --- | --- |
| Both boards pass `synth check` independently | **Not done.** Both pass `synth validate` (0 errors) and KiCad ERC (0 errors), but `synth check` is `fail`: the DRC stage is `unknown` because FreeRouting crashes before producing copper. | `agent-run/VERIFICATION_REPORT.md` sections 1, 5, 6 |
| Multiboard export reports 100 % matched pins | **Done.** 20/20, 100 %, no diagnostics. | `cargo test -p synth-cli --test bldc_stack_board`, `every_j_b2b_pin_is_matched_once` |
| Three negative mapping cases each fail | **Done.** Swap, drop and 12 V/3.3 V exchange are rejected; a source-level pin swap is also rejected. | the four other tests in `bldc_stack_board.rs` |
| Connector current table with derating | **Done.** 2.5 A/contact, 50 % bracket, highest pin 220 mA. | `README.md` |
| README "Not verified by Synth": dead time, shoot-through, loop inductance, EMI, motor transients | **Done.** | `README.md`, `agent-run/DESIGN_NOTES.md` |
| Deterministic reruns give identical hashes | **Not done.** No routed artifact; `SHA256SUMS` is local, not CI. | `agent-run/VERIFICATION_REPORT.md` section 7 |
| `qualified: false`, human gates false | **Done.** | `qualification.json` |
| New `[[board]]` row, class D, experimental | **Done.** | `qualification/matrix.toml` |
| agent-run folder | **Done.** Nine files. | `agent-run/` |

## Gate command output (verdict lines)

```
synth validate control.synth   -> exit 0, 0 errors, 21 warnings
synth validate power.synth     -> exit 0, 0 errors, 42 warnings
cargo test -p synth-cli --test bldc_stack_board -> 6 passed
kicad-cli sch erc control      -> 0 errors, 6 warnings
kicad-cli sch erc power        -> 0 errors, 15 warnings
synth export-kicad control     -> routing validation_failed (FreeRouting assert)
synth export-kicad power       -> killed after 28 min at ~96% CPU, no PCB
synth check control --json     -> fail (source pass, drc unknown)
synth check control --fab      -> fail (source pass, drc unknown, manufacturing fail)
```

## Gaps and assumptions

1. **No routed copper.** FreeRouting 2.4.1 crashes in this environment on the
   committed Class-B buck board too. The delivered control PCB is un-routed; the
   power PCB was never written. No Gerbers, drill files or fab package exist.
2. **Power PCB and BOM missing.** The power export wrote the schematic, project,
   symbol and library table but no PCB or BOM; it was killed after 28 minutes at
   about 96 % CPU. This is a deliverable gap.
3. **Board size.** The control outline is 61.99 x 36.97 mm, the height of J_B2B plus the
   exporter edge margin. The power sidecar uses the same holes and J_B2B in a
   60 x 50 record; it has no exported PCB.
4. **Part review.** The ten authored parts have `reviewed_by = ""` by design.
   The seed parts used here (AMS1117, THVD1450D, STM32G030K8, PTVS, 2N3904,
   SMBJ30A, generic passives) were not re-reviewed in this branch.
5. **LCSC numbers** for the Phoenix terminal blocks are C480516/C480520 (found
   with Firecrawl/JLCPCB). Generic passives still have no MPN.
6. **No hardware.** Nothing was powered, programmed or measured. The bring-up
   steps are written, not run.
7. **No thermal/SI/PI/EMC/safety analysis.** The AMS1117 warning is a
   worst-case datasheet estimate; the MOSFET junction temperatures are not
   computed.
8. **MCP server mismatch.** The MCP server in this session is the stale
   `/home/steve/work/synth-mcp-main` binary, so `synth_export_multiboard`
   reported every board as unknown. The branch's own `synth_mcp::call_tool` is
   what the test exercises; the stale server was not modified.
9. **Hashes.** `SHA256SUMS` was generated locally; no CI golden set exists.
