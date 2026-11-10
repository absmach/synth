# 3D models not in KiCad's stock library

## USB_C_Receptacle_HRO_TYPE-C-31-M-12.step

- Part: Korean Hroparts TYPE-C-31-M-12, LCSC C165948 (J1 on rtd-afe and usb-uart).
- Source: GrabCAD "TYPE-C-31-M-12", https://grabcad.com/library/type-c-31-m-12-1 (file `type-c-31-m-12-1.snapshot.5.zip`, downloaded 2026-11-10). The STEP header names the author only as "Author" (FreeCAD export, 2020-05-09). GrabCAD page terms apply; confirm them before any wider redistribution.
- 570 faces, 718 KB, body 8.95 x 7.9 mm, 3.2 mm high. It replaces the 29 KB simplified block in `rtd-afe/agent-run/step-models/`, which was built from datasheet dimensions.
- Alignment to KiCad's `Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12` footprint: the model origin is a corner of the body, so add the offset `(offset (xyz -4.475 -3.75 0))` and no rotation. Checked by rendering the RTD board with this model: the receptacle sits on its pads and shield tabs and the opening faces the board edge.
- Used by the committed STEP exports and 3D images of rtd-afe and usb-uart (rendered from a scratch copy of each board with this model path). Not yet used by the committed `.kicad_pcb` files, which still name the stock placeholder path because the exporter has no custom-model path (see each board's open items).
