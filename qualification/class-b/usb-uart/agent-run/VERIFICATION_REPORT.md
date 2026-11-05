> **Retrospective documentation reconstruction, not reproducible prompt-to-PCB evidence.** The exact prompt, ordered tool calls, model and tool versions, and export session report were not saved. This board is **not Class-B qualified** (`qualified: false`). Interface limits: USB is bench-only (25.11 mm DP/DN skew, no coupled pair, no controlled impedance, no independent SI review); RS-485 is short-cable, point-to-point or last-node bench use only (no A/B surge protection, 120 ohm termination always fitted). The JLCPCB DFM record is for the previous revision and the final files were not re-uploaded.

# Verification report

Copied from the committed reports and `../qualification.json`. No gate was re-run for this document. Overall: `qualified: false`.

## Reports
| File | Verdict |
| --- | --- |
| `../reports/release.json` (synth.release.v1, synth-cli 0.1.1) | release_ready true, production_status "production", routing state "routed" (FreeRouting 2.4.1). gate_scope: covers only Synth's routing and export gates; not a fabrication approval, qualification or component review. reviewer_state false for all 12 part ids. Signal integrity: 0 blocking, 23 warnings (W-SYNTH-DRC-016, layer change with no ground return via within 1.53 mm). Note: fabrication note records the U1.25 via-in-pad. |
| `../reports/usb_uart.drc.json` (KiCad 10.0.7) | 0 violations, 0 unconnected items |
| `../reports/usb_uart.erc.json` (KiCad 10.0.7) | 1 warning: label_multiple_wires; no errors |
| `../reports/usb_uart.routing.json` | state routed, 26 connected nets, 0 open nets, 273 segments, 43 vias |

## Gates in qualification.json
| Gate | passed |
| --- | --- |
| routing_and_drc | true |
| gnd_pour_connectivity | true |
| reproducible_from_clean_checkout | true (2026-10-27, three byte-identical exports; one machine only) |
| erc_dispositioned | false (author dispositions; human reviewer has not confirmed) |
| usb_si_pi_review | false (no independent review; author measurements: DP 38.63 mm, DN 13.52 mm, skew 25.11 mm, 91.4 percent over B.Cu GND, pair not coupled) |
| rf_review | false (no independent review) |
| rs485_termination_and_protection | false (R_TERM permanent, no A/B surge network, no approved operating envelope) |
| fabrication_review | false (via-in-pad at U1.25 needs fill and cap; JLCPCB not confirmed) |
| component_and_manufacturer_review | false (reviewer_state false for every part) |
| jlcpcb_dfm | false (stale: describes the previous revision; trace/space, drill, edge-clearance and via-in-pad findings not captured) |
| layout_clearances | false (J1 courtyard 0.01 mm to top edge, enclosure cutout fit unchecked) |

## Open items and failures
- 8 of 11 gates are false; no hardware test; no independent review of any part.
- Validate warnings (README.md, not re-run): E-SYNTH-POWER-004 x2, E-SYNTH-SCHEM-005 x2, W-SYNTH-ANOMALY-001, W-SYNTH-SUPPLY-002 x8, W-SYNTH-PART-UNVERIFIED x9.
- Routing is placement-sensitive: other J3/J4 or C4 shifts left DTR1, TNOW0, +3V3 or USB_DN unrouted (README.md).
- USB pair length mismatch of 25.11 mm is a routing artefact; judged harmless at full speed by the author only.
- Lite-On LED numbers second-hand; CH342F datasheet V1E may not be the newest; U1 EP footprint 2.6 mm vs drawing 2.4 mm not reconciled; parts availability and LCSC numbers not checked.
