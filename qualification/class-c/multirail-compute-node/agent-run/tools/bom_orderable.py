#!/usr/bin/env python3
"""bom_orderable.py <exported bom.csv> <sourcing.csv> <out.csv>

Joins the exported BOM with the sourcing table on (value, footprint name) and writes one row per
reference with manufacturer, MPN, package, rating, datasheet, distributor, order number and the page it was found on.
A reference with no sourcing row, or whose part the design names by MPN, is listed with
`status` = needs sourcing / from registry. Nothing is guessed: only rows in sourcing.csv are used.
"""
import csv, sys
bom, src, out = sys.argv[1:4]
table = {}
for r in csv.DictReader(open(src)):
    table.setdefault((r["value"], r["footprint"]), []).append(r)
    table.setdefault((r["mpn"], r["footprint"]), []).append(r)
rows, missing = [], []
for r in csv.DictReader(open(bom)):
    fp = r["kicad_footprint"].split(":")[-1]
    key = (r["value"], fp)
    cands = table.get(key) or table.get((r["mpn"], fp)) or []
    s = None
    for c in sorted(cands, key=lambda c: -len(c["desc"])):
        if not c["desc"] or c["desc"].lower() in r["description"].lower():
            s = c; break
    if s:
        rows.append({**{k: r[k] for k in ("refdes", "value", "kicad_footprint")}, "package": s["package"], "manufacturer": s["manufacturer"], "mpn": s["mpn"],
                     "rating": s["rating"], "datasheet": s["datasheet"],
                     "distributor": s["distributor"], "order_number": s["order_number"], "link": s["link"], "status": "sourced", "note": s["note"]})
    else:
        generic = r["kind"] in ("mounting_hole", "testpoint", "fiducial") or fp.startswith(("TestPoint", "MountingHole", "Fiducial"))
        rows.append({**{k: r[k] for k in ("refdes", "value", "kicad_footprint")}, "package": fp, "manufacturer": "", "mpn": r["mpn"], "rating": "", "datasheet": "", "distributor": "", "order_number": "",
                     "link": "", "status": "board feature, no part" if generic else "needs sourcing", "note": ""})
        if not generic: missing.append(r["refdes"])
# JLCPCB's BOM tool flags lines with different Comments that match one part ("Multiple lines in the BOM have been
# matched to the same part") and leaves them unconfirmed. Lines that name one part under one footprint therefore share
# one value, the longest of them; the exported value stays in the note.
values = {}
for row in rows:
    if row["mpn"] and row["status"] == "sourced":
        values.setdefault((row["mpn"], row["kicad_footprint"]), set()).add(row["value"])
for row in rows:
    group = values.get((row["mpn"], row["kicad_footprint"]), set())
    if len(group) > 1 and row["status"] == "sourced":
        shared = max(group, key=len)
        if row["value"] != shared:
            row["note"] = (row["note"] + "; " if row["note"] else "") + f"exported value {row['value']}"
            row["value"] = shared
fields = ["refdes", "value", "kicad_footprint", "package", "manufacturer", "mpn", "rating", "datasheet", "distributor", "order_number", "link", "status", "note"]
with open(out, "w", newline="") as f:
    w = csv.DictWriter(f, fields); w.writeheader(); w.writerows(sorted(rows, key=lambda x: x["refdes"]))
print(f"{len(rows)} rows, {len(rows) - len(missing)} sourced or feature, {len(missing)} need sourcing: {' '.join(missing)}")
