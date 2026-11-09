# VERIFICATION_REPORT: rtd-afe

> Historical, agent-written, from an earlier revision. The committed `reports/release.json`, README.md and QUALIFICATION.md are authoritative. The PR status is untrusted / `release_ready: false` (crystal unreviewed); `release.json` itself is historical output from source 01a77d54.

Copied from the committed `reports/` files and `QUALIFICATION.md`. Nothing was re-run for this document. Hardware: not run. The board is not fabricated, not powered, not measured.

## Gate verdicts
| Gate | Verdict | Source file |
| --- | --- | --- |
| KiCad DRC, all severities, refill zones | PASS: 0 errors, 0 warnings, 0 unconnected, `violations: []`, `schematic_parity: []` | `reports/rtd_afe.kicad-drc.json` (KiCad 10.0.7) |
| KiCad ERC | 0 errors, 2 warnings (open, dispositioned in QUALIFICATION.md as library-comparison, needs a human to confirm) | `reports/rtd_afe.kicad-erc.json` |
| Connectivity | PASS: `fabrication_ready: true`, 40 connected nets, 0 open, no missing footprints/nets/pads, `blocking_reasons: []` | `reports/rtd_afe.connectivity.json` |
| Routing | routed, FreeRouting 2.4.1, 371 segments, 67 vias recorded by the router (77 incl. source stitching per QUALIFICATION.md), 654.8 mm wire, 0 open nets | `reports/rtd_afe.routing.json` |
| Synth release | FAIL/OPEN: `production_status: untrusted`, `release_ready: false` (override `--allow-unverified-parts` for `ecs_120_20_33_tr`). `gate_scope` says this is not a fabrication approval or component review | `reports/release.json` |
| Synth signal integrity | 12 findings, 0 blocking (11 return-via advisories, 1 parallel run) | `reports/release.json` |
| pcbnew reopen / single GND plane | PASS per `check.sh` (`reopen ok; single GND plane`), recorded in QUALIFICATION.md; the script output itself is not a committed report | `check.sh`, QUALIFICATION.md |
| Via-in-pad | 1 via at (16.500, 22.300) mm, C_DV.2; order with filled and capped vias | `reports/rtd_afe.connectivity.json`, `reports/release.json` |

## ERC warnings (open)
1. `lib_symbol_issues`: symbol `Ferrite_Bead` not found in library `Device` (L2). Registry uses the old name.
2. `lib_symbol_mismatch`: `REF5025ID` differs from the library copy (U3). Cosmetic per QUALIFICATION.md.

## SI warnings (non-blocking only with --fine-pitch-return-via-advisory, which needs SI-review sign-off, not given; default settings: validation_failed, 12 blocking; open)
11 layer changes with no ground-return via within 0.64 mm (SPI_MOSI/CS/MISO, ADC_DRDY, ADC_START, QSPI_SD0/SD1/SD3, +3V3_RP_A, DVDD_1V1, USB_DN_C and others listed in `release.json`) and 1 parallel run (see `reports/release.json` for the net pair and length). No RTD, AIN or VREF finding. USB path mismatch 1.31 mm, no controlled impedance. None simulated.

## Synth validate warnings (QUALIFICATION.md, not committed as a report)
E-SYNTH-POWER-004 (VBUS 7 loads, 1 capacitor), W-SYNTH-ANOMALY-001, W-SYNTH-SUPPLY-002 x10, W-SYNTH-PART-UNVERIFIED (Y1), E-SYNTH-CONNECT-007 x2, E-SYNTH-ESD-001 x3 (no VBUS reverse protection, no CC TVS), E-SYNTH-SCHEM-003 x11, E-SYNTH-SCHEM-004/005.

## Part reviews (`reports/release.json` reviewer_state)
Reviewed (Steve Munene, 2026-10-27): ads124s08, ref5025, lp5907_3v3. Not reviewed: rp2350, w25q128_flash, usb_c_receptacle, ecs_120_20_33_tr, esd_usb, ferrite_bead_0603, header_1x3, header_1x4, mounting_hole_m2_5, r_generic_0603, c_generic_0603, c_generic_0805, l_generic_0805. The `ads124s08` symbol is synthetic.

## Open gates (all from QUALIFICATION.md "Left to a human")
- SI/PI/RF review by the designated reviewer: open; nothing simulated or measured.
- Metrology/analog: not ratiometric; IDAC accuracy and drift, REF5025 grade, PGA common-mode range at chosen gain, unused-input termination, filter vs data rate: open, no error budget.
- USB/ESD: no VBUS fuse/TVS/reverse protection, CC1/CC2 no TVS, D1/D2 no MPN, USB-C compliance unchecked: open.
- Mechanical: enclosure, screw-head clearance, connector overhang, header orientation unchecked: open.
- Test points: none fitted: open.
- Fabrication/DFM: no fab has seen the files; via-in-pad process not accepted by JLCPCB; fine-pitch capability unchecked: open.
- Firmware: does not exist: open.
- Independent sign-off: none; no second-person check, no hardware measurement: open.
- Reproducibility: committed board is one successful run, not proven byte-identical on re-run.

## Discrepancy found while copying (resolved)
QUALIFICATION.md's Reproducibility table and an inline PCB hash were stale against the committed files; `SHA256SUMS` matched the files. Commit `4db6fe0` replaced the table with a pointer to `SHA256SUMS` and corrected the inline PCB hash (`1e4803cf...043a`). Nothing was re-run for this document. Which run produced the committed reports: not verified beyond `release.json` `source_revision` `28f7f1ec3aef55fa2881fdfbfaad6c1b6896b562`.

