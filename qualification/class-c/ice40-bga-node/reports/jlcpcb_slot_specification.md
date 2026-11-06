# JLCPCB plated-slot specification, quoted against the 4 slot-width Danger items

The JLCDFM report of this package ([`jlcdfm_pcb_dfm_2026-11-06_13-03_0729fcfd37.pdf`](jlcdfm_pcb_dfm_2026-11-06_13-03_0729fcfd37.pdf), Gerber content hash `0729fcfd37`) scores 4 slots of 0.6 mm as Danger in its "Slot width check" of the drill file `ice40_bga_node.drl`. They are the four plated slots for the shield legs of the USB-C receptacle `J1` (Korean Hroparts TYPE-C-31-M-12, KiCad footprint `Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12`).

The slots in the drill file of this package, cut with the 0.6 mm tool (`T3C0.600`, routed with `G85`): two of 1.2 x 0.6 mm (centres 0.6 mm apart) and two of 1.7 x 0.6 mm (centres 1.1 mm apart). The board has six copper layers.

JLCPCB's capabilities page, read on 2026-11-06 (it shows no date or version; its footer reads "© 2026 JLCPCB.COM"):

| Item | JLCPCB's wording |
|---|---|
| Minimum plated slot width | "Min. Plated Slots Width: 2-layer: 0.5mm" and "Multi-layer: 0.35mm" |
| The same, in prose | "The minimum plated slot width is 0.5mm, which is drawn with a pad." |
| Slot length | "The length of the slot should be at least 2 times of the width." |
| How a plated slot is made | "Plated slot is made by drill tools." |
| Tolerance | "Slot Hole size Tolerance: Plated: +0.13 / -0.08 mm" |

The slots are 0.6 mm wide, above the 0.35 mm multilayer minimum and above the 0.5 mm two-layer minimum. The shorter slots are 1.2 mm long, which is 2 times their width, and the longer ones are 1.7 mm. Source: [capabilities](https://jlcpcb.com/capabilities/Capabilities).

The capabilities page does not say what JLCDFM's slot check applies or what it does with a slot narrower than the minimum, and the connector's datasheet could not be read by the page reader (the LCSC link redirects to a page with no text), so the slot sizes above come from the KiCad footprint as written into the drill file.

The decision to carry these 4 items with this specification attached is Jeff Mboya's.
