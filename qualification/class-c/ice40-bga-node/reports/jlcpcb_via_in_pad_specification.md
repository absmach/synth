# JLCPCB via-in-pad specification, quoted against the 25 annular-ring Danger items

The JLCDFM report of this package ([`jlcdfm_pcb_dfm_2026-11-06_13-03_0729fcfd37.pdf`](jlcdfm_pcb_dfm_2026-11-06_13-03_0729fcfd37.pdf), Gerber content hash `0729fcfd37`) scores 25 annular rings of 0.07 mm as Danger. They are the 0.35/0.2 mm vias in the BGA pads of `U1`: a 0.2 mm drill in a 0.35 mm land, a ring of 0.075 mm. The report does not say which process it assumes; none of the JLCPCB pages read describes JLCDFM's annular-ring check or its thresholds.

The board has six copper layers and `requires via_in_pad`, so JLCPCB's via-in-pad process for six layers applies. The pages below were read on 2026-11-06. Each quotation is the longest verbatim fragment the reader could return.

| Item | JLCPCB's wording | Page |
|---|---|---|
| Process on six layers | "JLCPCB via-in-pad on 6- 20 layer PCBs are upgraded to POFV (Plated Over Filled Via) for free" | [news, Nov 02, 2022](https://jlcpcb.com/news/free-via-in-pad-6-20-layer-pcbs-pofv) |
| Annular ring of the process | "Annular ring: 0.05 mm minimum, 0.075 mm preferred" (under "3. POFV Design Requirements") | the same page |
| Via hole size of the process | "Via hole size: 0.2 to 0.5 mm" | the same page |
| Pad land for a filled via | "design the solder pad (outer diameter) to be 0.35mm or larger" | [BGA design guidelines, last updated Sep 09, 2026](https://jlcpcb.com/help/article/243-BGA-Design-Guidelines---PCB-Layout-Recommendations-for-BGA-packages) |
| Process on six layers and above | "This process is the default for 6-layer and above multilayer boards." | [capabilities](https://jlcpcb.com/capabilities/Capabilities) |

The vias flagged are a 0.2 mm hole (inside the 0.2 to 0.5 mm range) with a 0.075 mm ring (the process's preferred value, above its 0.05 mm minimum) in a 0.35 mm land (the BGA guide's minimum).

The capabilities page gives a different figure for ordinary plated holes: a PTH annular ring of 0.20 mm or more, and 0.15 mm as the absolute minimum for 1 oz multilayer. That figure is not the POFV process's, and the pages do not say which figure JLCDFM applies; the Danger items follow the ordinary-PTH reading.

The pages say nothing of a distance between a via and a plated through hole beyond the POFV page's "Should be placed more than 0.45 mm from regular PTHs or NPTHs".

The decision to carry these 25 items with this specification attached is Jeff Mboya's.
