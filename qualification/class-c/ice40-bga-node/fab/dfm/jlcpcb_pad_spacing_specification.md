# JLCPCB spacing specification, quoted against the pad-spacing Danger item

The JLCDFM reports of this package ([`jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf`](jlcdfm_pcb_dfm_2026-11-10_20-30_ceeb7444dd.pdf), Gerber content hash `ceeb7444dd`) and of the earlier one ([`jlcdfm_pcb_dfm_2026-11-06_14-42_99da25a419.pdf`](jlcdfm_pcb_dfm_2026-11-06_14-42_99da25a419.pdf), hash `99da25a419`) score one "Pad spacing" measurement of 0.1 mm as Danger on the copper layers.

Measured on the delivered board (`export/ice40_bga_node.kicad_pcb`, the board the Gerbers come from): the closest pair is pad 14 of `U5` and a 0.6/0.3 mm via at (17.80, 42.30) mm, with a copper gap of 0.100 mm. Both are on net `CFG_SS_B`: the via is the layer change of the pad's own connection. The next closest gaps are 0.125 mm (pads of `J1` and `U5`, each against a via of another net) and 0.126 mm (`C1` pad 2 against a via).

JLCPCB's rigid PCB capabilities page (`https://jlcpcb.com/capabilities/PCB-capabilities`) and its six-layer page (`https://jlcpcb.com/6-layer-pcb`), read on 2026-11-06 (neither shows a date or version; the footers read "© 2026 JLCPCB.COM"):

| Item | JLCPCB's wording |
|---|---|
| Six-layer minimum trace width and spacing | "Minimum trace width and spacing": "3.5mil (0.09mm)" |
| Track width and spacing, 1 oz | "Min. track width and spacing (1 oz)": "0.10 / 0.10 mm (4 / 4 mil)"; description "Multilayer: 0.09 / 0.09 mm (3.5 / 3.5 mil)" |
| Pad to track | "Pad to track clearance (1oz)": "0.1mm" |
| Pad to pad | "SMD pad to pad clearance (different nets)": "0.15mm" |

The measured gap of 0.100 mm is above the 0.09 mm multilayer spacing minimum and equal to the 0.1 mm pad-to-track figure. The pad-to-pad row names different nets; this pair is one net, so that row does not apply to it. JLCPCB's pages give no via-land-to-pad clearance, and do not say what JLCDFM's "Pad spacing" check applies or how it grades a via land against a pad.

The pair cannot be moved without changing the routed board, and with it the Gerber hash that the DFM report, quote and BOM matching are tied to.

The decision to carry this item with this specification attached is Jeff Mboya's to state.
