# Disposition of the JLCDFM PCB DFM Danger items, Gerber content hash `ceeb7444dd`

[`jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf`](jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf) is the JLCDFM report generated on 2026-11-10 at 20:30:26 for the Gerber zip of `fab/` (content hash `ceeb7444dd`): six layers, 5.6 x 4.5 cm, PCB DFM ticked, SMT DFM not ticked. It scores 27 items as Danger, in four checks. The same four checks carry the same Danger counts in the report of the earlier package (`jlcdfm_pcb_dfm_2026-11-06_14-42_99da25a419.pdf`).

| JLCDFM check | Danger | What the board shows | Specification |
|---|---|---|---|
| Pad spacing (0.1 mm) | 1 | The closest pair on the delivered board (`export/ice40_bga_node.kicad_pcb`) is pad 14 of `U5` and the 0.6/0.3 mm via at (17.80, 42.30) mm, a copper gap of 0.100 mm; both are on net `CFG_SS_B`, the via being the layer change of the pad's own connection | [`jlcpcb_pad_spacing_specification.md`](jlcpcb_pad_spacing_specification.md) |
| Annular ring (0.08 mm) | 19 | The board has 18 vias of 0.35/0.2 mm (a ring of 0.075 mm) in the pads of the 0.8 mm BGA `U1`, the filled-and-capped via of JLCPCB's via-in-pad process. The report does not name its 19 items, so the nineteenth is not identified | [`jlcpcb_via_in_pad_specification.md`](jlcpcb_via_in_pad_specification.md) |
| Via to pad (0 mm) | 3 | The report does not name them | [`jlcpcb_via_in_pad_specification.md`](jlcpcb_via_in_pad_specification.md) |
| Slot width (0.6 mm) | 4 | The four plated 0.6 mm slots for the shield legs of the USB-C receptacle `J1` | [`jlcpcb_slot_specification.md`](jlcpcb_slot_specification.md) |

The specification files quote JLCPCB's own pages and were read against the report of the earlier package; the counts above are those of this report.
