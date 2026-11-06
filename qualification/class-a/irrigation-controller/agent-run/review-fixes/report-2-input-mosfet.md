# REPORT_Q1: Q1 changed to AO3401A (30 V)

## What changed
- `irrigation_controller.synth`: Q1 part `p_mosfet_si2301` -> `p_mosfet_ao3401a`. Q_E1 and Q_E2 are unchanged (still Si2301).
- Two source-note edits in the POWER INPUT group: the Q1 description names the AO3401A, and a new line records the gate check: the 5.1 V zener D_QG across source and gate limits Vgs to about 5.1 V, inside the 12 V gate-source rating. The 21.5 V worst-case TVS clamp is below the 30 V Vds rating.
- The placement sidecar (`irrigation_controller.placement.layout.toml`) is untouched. No part was moved.

## Registry part authored
`user-registry/p_mosfet_ao3401a.synth.toml` (synth_author_part): SOT-23, kicad_symbol, kicad_footprint, footprint_dimensions and pins g=1, s=2, d=3 copied from the Si2301 reference. Description "Alpha and Omega AO3401A P-channel MOSFET, 30 V, 4 A, SOT-23", mpn AO3401A, max_voltage_v 30, max_current_ma 4000, datasheet_url set. No lcsc_pn.
The first attempt was rejected ("authored parts cannot claim seed provenance"). It was accepted once the `[provenance]` block carried `source = "authored"`.

## Result (fresh folders `export-try1/` and final `export/`, identical results)
- release_ready: true
- route_complete: true (43 of 43 nets connected, 0 open)
- KiCad DRC (from the export gate): 0 errors, 0 unconnected, 108 warnings
- No via in a pad, no undersized tracks or vias, `fabrication_ready` true, `blocking_reasons` empty
- `allow_incomplete` was not used.

## Not done or not verified
- Outline size: I did not measure it. The sidecar is unchanged, so it should still be the 75.1 x 65.4 mm in the brief, but I did not read it from Edge.Cuts.
- Neither the .synth nor the sidecar was copied into `export/`; both are in the workspace root beside it. Those files are the inputs used for the export.
- The 108 DRC warnings were not itemised.
- The router report shows `engine_policy_verdict: "rejected"` with a claimed 73 vias against 77 in the delivered board. Synth's own validation passed, but I did not investigate it.
- The first validate call with `registry_path` set to the user registry returned unknown-part errors because it replaced the shipped registry. Exporting without `registry_path` worked, and I used that for both exports.
- Datasheet values (30 V, 12 V Vgs, 4 A) are taken from your brief, not read from the datasheet.
- 8 schematic readability warnings and several info items remain, as before.
