#!/usr/bin/env python3
"""merge_groups_remove_part.py <board.synth> <sidecar.toml>

Mechanical source edit applied by the orchestrator while the droplet agent was unavailable
(weekly usage limit). It changes group membership and removes one part; it moves nothing.
  * removes component TP_V1 (source line, its net pin, its sidecar entry, the note sentence)
  * folds the 18 groups the review-fix and fill runs produced into 13, so the schematic
    floorplan fits one page again. A merged group keeps every component and every note string.
"""
import os, re, sys
src_path, side_path = sys.argv[1:3]
KEEP_TP_V1 = os.environ.get("KEEP_TP_V1") == "1"
text = open(src_path).read()

if not KEEP_TP_V1:
    text = re.sub(r"^\s*component TP_V1:.*\n", "", text, flags=re.M)
    text = text.replace(" TP_V1.p1", "")
    text = text.replace(" TP_V1 is a test point on VALVE1_SW.", "")
    side = open(side_path).read()
    side = re.sub(r"\[components\.TP_V1\]\n(?:[^\[\n].*\n?)*", "", side)
    open(side_path, "w").write(side)

group_re = re.compile(r'^  group "([^"]+)" color "([^"]+)" \{\n(.*?)^  \}\n', re.S | re.M)
STR = re.compile(r'"((?:[^"\\]|\\.)*)"')

def parse_body(body):
    comps, notes, i = [], [], 0
    lines = body.split("\n")
    while i < len(lines):
        l = lines[i]
        if l.strip().startswith("component "):
            comps.append(l); i += 1
        elif l.strip().startswith("notes "):
            block = [l]
            depth = l.count("{") - l.count("}")
            while depth > 0:
                i += 1; block.append(lines[i]); depth += lines[i].count("{") - lines[i].count("}")
            inner = "\n".join(block)
            inner = inner[inner.index("{") + 1: inner.rindex("}")]
            notes.extend(STR.findall(inner)); i += 1
        else:
            if l.strip(): raise SystemExit(f"unrecognised line in a group: {l!r}")
            i += 1
    return comps, notes

groups, order = {}, []
for m in group_re.finditer(text):
    comps, notes = parse_body(m.group(3))
    groups[m.group(1)] = dict(color=m.group(2), comps=comps, notes=notes, span=(m.start(), m.end()))
    order.append(m.group(1))

def ref(l): return re.match(r"\s*component (\w+):", l).group(1)
def take(src, refs, dst):
    keep = [l for l in groups[src]["comps"] if ref(l) not in refs]
    groups[dst]["comps"] += [l for l in groups[src]["comps"] if ref(l) in refs]
    groups[src]["comps"] = keep
def fold(src, dst):
    groups[dst]["comps"] += groups[src]["comps"]; groups[dst]["notes"] += groups[src]["notes"]
    groups[src]["gone"] = True

take("12V BULK AND SNUBBER", {"R_SN1", "C_SN1"}, "VALVE 1")
fold("VALVE 2 SNUBBER", "VALVE 2")
fold("12V BULK AND SNUBBER", "12V RAIL DECOUPLING")
fold("3V3 EXTRA DECOUPLING", "3V3 RAIL DECOUPLING")
fold("RAIL INDICATORS", "LEDS")
fold("MOUNTING", "RAIL TEST POINTS")
groups["RAIL TEST POINTS"]["new_name"] = "TEST POINTS AND MOUNTING"

out, cursor = [], 0
for name in order:
    g = groups[name]; s, e = g["span"]
    out.append(text[cursor:s]); cursor = e
    if g.get("gone"): continue
    notes = ""
    if g["notes"]:
        notes = '    notes "" {\n' + "\n".join(f'      "{n}"' for n in g["notes"]) + "\n    }\n"
    out.append(f'  group "{g.get("new_name", name)}" color "{g["color"]}" {{\n' + "\n".join(g["comps"]) + "\n" + notes + "  }\n")
out.append(text[cursor:])
open(src_path, "w").write("".join(out))
print("groups now:", [g.get("new_name", n) for n, g in groups.items() if not g.get("gone")])
