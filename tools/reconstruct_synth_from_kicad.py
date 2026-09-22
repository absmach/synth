#!/usr/bin/env python3
"""Reconstruct a Synth source and placement sidecar from a KiCad export.

This is intentionally conservative: it preserves the KiCad netlist's actual
connectivity, while collapsing duplicate power/USB aliases to the logical pins
used by the Synth registry.
"""
from __future__ import annotations

import argparse
import csv
import re
from pathlib import Path


def blocks(text: str, marker: str):
    start = 0
    while True:
        pos = text.find(marker, start)
        if pos < 0:
            return
        depth = 0
        end = pos
        for i in range(pos, len(text)):
            if text[i] == "(":
                depth += 1
            elif text[i] == ")":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        yield text[pos:end]
        start = end


def field(block: str, key: str) -> str | None:
    m = re.search(rf'\({re.escape(key)}\s+"([^"]*)"\)', block)
    return m.group(1) if m else None


def pin_for(ref: str, pin: str, function: str | None) -> str:
    if ref == "J1":
        f = (function or "").split("_")[0].lower()
        f = f.replace("+", "p").replace("-", "n")
        return {"vbus": "vbus", "gnd": "gnd", "dp": "dp", "dn": "dn",
                "cc1": "cc1", "cc2": "cc2", "sbu1": "sbu1", "sbu2": "sbu2",
                "shld": "shld"}.get(f, "gnd" if pin in {"A12", "B12", "A1", "B1"} else "vbus" if pin[0] in "AB" and pin[1:] in {"4", "9"} else "gnd")
    if ref == "U1":
        f = function or pin
        f = f.replace("~{", "").replace("}", "")
        if "/" in f:
            f = f.split("/")[0] + "_" + f.split("/")[1].split("_")[0]
        f = re.sub(r"_\d+$", "", f)
        return f
    if ref == "U2":
        f = re.sub(r"_\d+$", "", function or pin).lower()
        return {"vi": "vin", "vo": "vout", "gnd": "gnd"}.get(f, f)
    if ref == "U3":
        f = (function or "").lower()
        if "cs" in f: return "cs"
        if "clk" in f: return "clk"
        if "io_0" in f or "di/io" in f: return "io0"
        if "io_1" in f or "do/io" in f: return "io1"
        if "io_2" in f or "wp" in f: return "io2"
        if "io_3" in f or "hold" in f: return "io3"
        if "vcc" in f: return "vcc"
        if "gnd" in f: return "gnd"
        return {"1": "cs", "2": "io1", "3": "io2", "4": "gnd", "5": "io0", "6": "clk", "7": "io3", "8": "vcc"}.get(pin, pin)
    if ref == "D4":
        return {"1": "VDD", "2": "DOUT", "3": "VSS", "4": "DIN"}.get(pin, pin)
    if ref in {"D2", "D3"}:
        return "io" if pin == "1" else "gnd"
    if ref.startswith("D") and ref != "D4":
        return "anode" if pin == "1" else "cathode"
    if ref == "Y1":
        return "p1" if pin == "1" else "p2"
    if ref.startswith("J") and ref != "J1":
        return "Pin_" + pin
    return "p" + pin


def part_id(ref: str, value: str, footprint: str) -> str:
    if ref == "U1": return "rp2350a_qfn60"
    if ref == "U2": return "ams1117_3v3"
    if ref == "U3": return "w25q128_flash"
    if ref == "J1": return "usb_c_receptacle"
    if ref in {"J2", "J3"}: return "header_1x20"
    if ref == "J4": return "header_1x3"
    if ref == "D4": return "ws2812b_5050"
    if ref in {"D2", "D3"}: return "esd_usb"
    if ref == "D1": return "led_green_0603"
    if ref == "D5": return "led_red_0603"
    if ref == "F1": return "polyfuse"
    if ref == "L1": return "l_generic_0805"
    if ref == "L2": return "ferrite_bead_0603"
    if ref.startswith("C"):
        return "c_generic_0805" if "0805" in footprint else "c_generic_0603" if "0603" in footprint else "c_generic_0402"
    if ref.startswith("R"): return "r_generic_0603"
    if ref.startswith("SW"): return "spst_tactile"
    if ref == "Y1": return "xtal_generic"
    raise ValueError(f"unmapped component {ref} {value} {footprint}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("netlist", type=Path)
    ap.add_argument("bom", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--pcb", type=Path, required=True)
    args = ap.parse_args()
    net_text = args.netlist.read_text()
    bom = {r["refdes"]: r for r in csv.DictReader(args.bom.open(newline=""))}
    comps = []
    for b in blocks(net_text, "\t\t(comp"):
        ref, value, fp = field(b, "ref"), field(b, "value"), field(b, "footprint")
        if ref and ref in bom:
            comps.append((ref, part_id(ref, value or "", fp or "")))
    nets = []
    for b in blocks(net_text, "\t\t(net"):
        name = field(b, "name") or ""
        if name.startswith("unconnected-"):
            continue
        ns = []
        for n in blocks(b, "\t\t\t(node"):
            ref, pin = field(n, "ref"), field(n, "pin")
            fun = field(n, "pinfunction")
            if ref and pin:
                logical = (ref, pin_for(ref, pin, fun))
                if logical not in ns: ns.append(logical)
        if len(ns) > 1:
            nets.append(ns)
    lines = ['board "rp2350_devboard_reconstructed" {', '  layers 4', '  manufacturer "jlcpcb"', '']
    for ref, pid in sorted(comps):
        lines.append(f'  component {ref}: device "{pid}"')
    lines.append('')
    for ns in nets:
        # Duplicate USB power/ground aliases collapse to one registry pin.
        # Keep the de-duplication local to this net; a logical pin may not be
        # reused across unrelated nets, but can legitimately appear in a
        # second KiCad alias net while reconstructing a connector.
        ns = list(dict.fromkeys(ns))
        if len(ns) < 2: continue
        for a, b in zip(ns, ns[1:]):
            lines.append(f'  connect {a[0]}.{a[1]} -> {b[0]}.{b[1]}')
    lines += ['', '  placement_hint { component: "J1" edge: top priority: hard }', '  placement_hint { component: "J2" edge: left priority: hard }', '  placement_hint { component: "J3" edge: right priority: hard }', '  placement_hint { component: "U1" region: centre priority: hard }', '  placement_hint { component: "U3" near: "U1" priority: soft }', '  placement_hint { component: "U2" near: "J1" priority: soft }', '}']
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(lines) + "\n")

    # Recover the exact successful-run placement from the Synth pre-routing PCB.
    side = args.out.with_suffix(".layout.toml")
    out = ['schema_version = 2', '']
    for b in blocks(args.pcb.read_text(), "(footprint"):
        mref = re.search(r'\(property\s+"Reference"\s+"([^"]+)"', b, re.S)
        ref = mref.group(1) if mref else None
        at = re.search(r"\n\t\t\(at ([^\s)]+) ([^\s)]+)(?: ([^\s)]+))?\)", b)
        if not ref or not at: continue
        rot = at.group(3) or "0"
        out += [f'[components.{ref}]', f'x = {at.group(1)}', f'y = {at.group(2)}', f'rotation = {rot}', 'source = "reconstructed"', 'priority = "hard"', '']
    side.write_text("\n".join(out))
    print(args.out)
    print(side)
    print(f"components={len(comps)} nets={len(nets)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
