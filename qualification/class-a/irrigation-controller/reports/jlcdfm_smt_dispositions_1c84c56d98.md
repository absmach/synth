# Disposition of the JLCDFM SMT checks, Gerber content hash `1c84c56d98`

For the report of 2026-11-06 13:59 ([`jlcdfm_pcb_and_smt_dfm_2026-11-06_13-59_1c84c56d98.pdf`](jlcdfm_pcb_and_smt_dfm_2026-11-06_13-59_1c84c56d98.pdf), run in JLCPCB's assembly flow on `fab/`'s Gerbers, BOM and CPL, summary in [`jlcdfm_smt_dfm_2026-11-06_1c84c56d98.md`](jlcdfm_smt_dfm_2026-11-06_1c84c56d98.md)).

The report prints one sample screenshot per check and names no component. In the samples the flagged items are right-angle through-hole connectors of the JST family (`J_VALVE1` and `J_VALVE2`, S2B-XH-A-1, 2.5 mm pitch; `J_FLOW`, `J_SOIL1`, `J_SOIL2` and `J_I2C`, S3B-PH-K-S and S4B-PH-K-S, 2 mm pitch) and the SOT-23 parts beside them (one sample is labelled `Q_V2`). Each check compares a package model that JLCPCB holds for the part with the pads and holes of this board, so what follows is what this package's own checks show for each kind of finding. It is not a per-component list.

| JLCDFM check | What the package's own evidence shows |
|---|---|
| Component collision, component spacing | KiCad's DRC with the library tables reports 0 courtyard overlaps ([`kicad_drc.json`](kicad_drc.json): its only findings are 121 silkscreen `lib_footprint_mismatch` warnings); the STEP model of every part reopened in OpenCASCADE has identities and positions equal to the PCB to 0.0000 mm and no body beyond its courtyard by more than 0.25 mm ([`step_verify.txt`](step_verify.txt)) |
| Component to board edge distance | the eight connectors are flush with the board edge by design: courtyards 0.00 to 0.23 mm from Edge.Cuts, `J_SWD` 0.23 mm inboard ([README](../README.md), "Layout") |
| Component through-hole (misalignment), lead to hole distance, missing hole for component pin | the drill file and the STEP model agree: 13 holes of 0.75 mm, 2 of 0.8 mm, 11 of 1.0 mm and 4 of 2.5 mm in both ([`step_verify.txt`](step_verify.txt)); every through-hole footprint is KiCad's stock library footprint, with pads, courtyard and fabrication outline identical to the library file ([`footprint_diff.txt`](footprint_diff.txt)) |
| Pin without pad, pin outer edge, lead area overlapping pad | the same stock library footprints: the pads are the library's, and the placement of each part is the CPL of `fab/` |

The through-hole connectors and the hand-fitted lines (`C_BULK`, `J_SWD`, `U4`, `U5`) have their dispositions in [`assembly_dispositions_1c84c56d98.md`](assembly_dispositions_1c84c56d98.md).

The report was run once, on this package. Which of the 58 items belong to which component is not in the PDF.
