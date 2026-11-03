# Verification report: BLDC stack (SYN-408)

All commands were run from the repository root on 2026-11-03 with an empty
`XDG_DATA_HOME` and `KICAD_SYMBOL_DIR=/usr/share/kicad/symbols`,
`KICAD_FOOTPRINT_DIR=/usr/share/kicad/footprints`. KiCad 10.0.7, FreeRouting
2.4.1, OpenJDK 21.0.12.1, Synth 0.1.1.

## 1. Source validation

```
$ synth validate qualification/class-d/bldc-stack/control.synth
exit 0; 0 errors, 21 warnings
  10 W-SYNTH-SUPPLY-002
   5 W-SYNTH-PART-UNVERIFIED
   2 E-SYNTH-SCHEM-005
   1 W-SYNTH-THERMAL-001
   1 E-SYNTH-POWER-004
   1 W-SYNTH-ANOMALY-001
   1 E-SYNTH-SCHEM-004

$ synth validate qualification/class-d/bldc-stack/power.synth
exit 0; 0 errors, 42 warnings
  22 W-SYNTH-PART-UNVERIFIED
   6 E-SYNTH-SCHEM-005
   5 W-SYNTH-SUPPLY-002
   5 E-SYNTH-SCHEM-003
   3 E-SYNTH-SCHEM-001
   1 W-SYNTH-ANOMALY-001
```

The `W-SYNTH-PART-UNVERIFIED` warnings are intentional: every authored part has
`reviewed_by = ""`.

## 2. Registry qualification

```
$ synth registry qualify --part <id> --no-fail
header_2x10, header_2x10_socket, bsc030n08ns5, irf4905, ir2101,
mcp6022_soic, terminal_block_mkds_2pin, terminal_block_mkds_3pin,
c_electrolytic_470uf_50v, bzt52c15
```

All pass `pin_numbering`, `pin_pad_coverage`, `pad_pin_coverage`,
`footprint_side`, `symbol_pin_coverage`, `declared_dimensions`,
`package_dimensions`, `pin_classification` and `three_d_model`. All are blocked
on `E-SYNTH-QUAL-008` ("part has no reviewer"), as required.

## 3. Multiboard acceptance

```
$ cargo test -p synth-cli --test bldc_stack_board
running 6 tests
test every_j_b2b_pin_is_matched_once ... ok
test swapping_two_mapped_pins_is_rejected ... ok
test a_dropped_mapping_is_rejected ... ok
test a_12v_pin_mapped_to_a_3v3_pin_is_rejected ... ok
test a_control_source_with_swapped_connector_pins_is_rejected ... ok
test the_connector_and_mounting_holes_line_up ... ok
test result: ok. 6 passed; 0 failed
```

The good mapping: `status = ok`, 20 total, 20 matched, 100 %, no diagnostics.
The mechanical test: J_B2B and H1-H4 at identical coordinates, 0.000 mm X/Y
mismatch, equal rotation.

## 4. Export and routing

```
$ SYNTH_SOURCE_REVISION=$(git rev-parse HEAD) synth export-kicad \
    qualification/class-d/bldc-stack/control.synth --out <out> \
    --allow-unverified-parts --validate-erc --router free-routing
routing <out>/bldc_control.synth.kicad_pcb with freerouting
validation_failed: freerouting routing failed at route stage (crashed):
  tools/freeroute_autoroute.py exited 1:
  ./kicad/include/properties/property.h(607): assert "m_choices.GetCount() > 0"
  failed in PROPERTY_ENUM(): No enum choices defined
kicad-cli pcb drc: errors 0, unconnected pads 97, warnings 112
wrote release.json
```

The same command on the committed `qualification/class-b/buck/buck.synth` fails
with the identical assert, so the failure is environmental, not caused by this
board. No copper was produced. `release_ready` is false and `production_status`
is `untrusted`.

The power-board export was killed after 28 minutes at about 96 % CPU with no
PCB written; it was stuck in the export path, not in an external router. Its schematic, project, symbol and library table were
written and are committed. The BOM and PCB are missing. This is a failure, not
a pass.

## 5. KiCad ERC

```
$ kicad-cli sch erc --severity-all --format json ... control
Found 6 violations: 0 errors, 6 warnings
  4 isolated_pin_label
  2 no_connect_connected

$ kicad-cli sch erc --severity-all --format json ... power
Found 15 violations: 0 errors, 15 warnings
  14 lib_symbol_mismatch (MCP6022, BSC030N08NS5, 2N3904)
  1 label_multiple_wires (GATE_REV)
```

The `lib_symbol_mismatch` warnings come from flattened `extends` symbols; the
embedded copy is the one the project uses. They are recorded, not suppressed.

## 6. `synth check`

```
$ synth check qualification/class-d/bldc-stack/control.synth --json
status: fail
  source: pass
  drc: unknown (parse_error: true)

$ synth check qualification/class-d/bldc-stack/control.synth --fab --json
status: fail
  source: pass
  drc: unknown
  manufacturing: fail
```

The DRC stage cannot run because no copper exists and the FreeRouting adapter
crashes. The fab stage fails for the same reason. These are the recorded
verdicts; no check was weakened or removed.

## 7. Hashes

`SHA256SUMS` lists the sources, sidecars, mappings, KiCad projects and reports.
The values were produced locally with `sha256sum` on 2026-11-03. No CI run has
produced a golden set, and the power PCB does not exist. The control PCB hash is
the hash of the un-routed board.

## 8. What passed and what did not

Passed:
- Both sources lower and pass Synth ERC with zero errors.
- All 20 J_B2B pins map once; the good mapping is 100 %.
- All four negative variants are rejected.
- The connector and mounting holes align to 0.000 mm.
- Every authored part passes the registry's structural checks.
- KiCad ERC has zero errors on both schematics.

Failed or incomplete:
- No copper: FreeRouting crashes on this host, including on the committed buck
  board.
- The power PCB and BOM are missing because the export did not finish.
- DRC cannot be run to a verdict; the un-routed control board has 97
  unconnected pads.
- `synth check` is `fail`; `--fab` is `fail`.
- No thermal, SI/PI, EMC, safety, fab or hardware test was done.

## 9. Datasheets fetched with Firecrawl (2026-11-03)

The terminal-block and connector evidence was a distributor listing; it was
replaced with Firecrawl extracts:

```
firecrawl scrape https://www.phoenixcontact.com/us/products/1715721/pdf
firecrawl scrape https://www.phoenixcontact.com/us/products/1715734/pdf
firecrawl scrape https://www.lcsc.com/product-detail/C5116480.html
firecrawl scrape https://www.lcsc.com/product-detail/C30867.html
firecrawl scrape https://www.onsemi.com/pdf/datasheet/2n3903-d.pdf
firecrawl scrape --alexandria jlcpcb-com/electronic-components/search_parts
  {"query":"1715721"} -> C480516
  {"query":"1715734"} -> C480520
```

Results: Phoenix MKDS 1,5/2-5,08 and 3-5,08 are 17.5 A, 400 V (III/2), 5.08 mm,
1.5 mm², 0.9 x 0.9 mm solder pin, linear pinning; LCSC C480516/C480520. The
2 x 10 header/socket are 2.5 A/contact, 2.54 mm (the LCSC "datasheet" PDF is a
3D model; the product page has the table). The 2N3904 is 40 V VCEO and 200 mA,
so the registry entry was corrected from 5 V/100 mA. The extracts are in
`datasheets/`. No reviewer was added; `reviewed_by` stays empty.
