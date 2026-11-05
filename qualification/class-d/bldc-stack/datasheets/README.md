# Datasheet extracts: BLDC stack (SYN-408)

These are Firecrawl text extracts, fetched on 2026-11-03, for the parts whose
datasheet evidence was missing or only a distributor listing before. The PDFs
themselves are not committed; the source URL and the extract are.

| File | Part | Source | What was read |
| --- | --- | --- | --- |
| `phoenix-1715721.md` | Phoenix Contact MKDS 1,5/2-5,08 (1715721) | <https://www.phoenixcontact.com/us/products/1715721/pdf> | 2 positions, 5.08 mm, 17.5 A, 400 V (III/2), 1.5 mm², 0.9 x 0.9 mm solder pin, 3.5 mm pin length, linear pinning |
| `phoenix-1715734.md` | Phoenix Contact MKDS 1,5/3-5,08 (1715734) | <https://www.phoenixcontact.com/us/products/1715734/pdf> | 3 positions, same ratings and pin geometry |
| `zhouri-2.54-2x10.md` | ZHOURI 2.54-2*10 (C5116480) | <https://www.lcsc.com/product-detail/C5116480.html> | 2 x 10, 2.54 mm, 2.5 A/contact, 20 pins, brass/gold, square pins |
| `boomele-2.54-2x10p.md` | BOOMELE 2.54-2*10P (C30867) | <https://www.lcsc.com/product-detail/C30867.html> | 2 x 10 socket, 2.54 mm, 2.5 A/contact, 20 pins, square holes |
| `onsemi-2n3904.md` | onsemi 2N3903/2N3904 | <https://www.onsemi.com/pdf/datasheet/2n3903-d.pdf> | VCEO 40 V, VCBO 60 V, VEBO 6 V, IC 200 mA, TO-92 pin 1 emitter / 2 base / 3 collector |

## What the fetch changed

- The Phoenix parts now have `lcsc_pn = "C480516"` (2-pin) and `"C480520"`
  (3-pin). The JLCPCB catalogue matches both MPNs exactly. JLC lists 250 V while
  Phoenix lists 400 V (III/2); the 24 V rail is below both, and the Phoenix
  figure is the one in the registry.
- The 2 x 10 connector ratings are confirmed as 2.5 A/contact. The LCSC
  "datasheet" PDF is a 3D model, not a drawing; the spec table is the evidence.
- The 2N3904 registry entry was corrected from 5 V/100 mA to 40 V/200 mA. The
  collector in this design sees 12 V, which the old field would have made look
  out of range.

## What the fetch did not change

- No reviewer: all authored parts still have `reviewed_by = ""`, so
  `W-SYNTH-PART-UNVERIFIED` and `E-SYNTH-QUAL-008` still fire.
- No dimensional drawing was read for the Phoenix blocks or the 2 x 10
  connectors; the text extract has the pin count, pitch and ratings, not the
  land pattern. The KiCad footprints remain the geometry source.
- The terminal-block pin numbering is "linear pinning, one solder pin per
  potential"; the extract does not contain a numbered drawing, so pins 1..N are
  the linear order.
