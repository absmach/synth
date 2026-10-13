# Dual-USB compute board — #40 case 3

Two RP2350s on a six-layer stackup, each on its own native USB, each pair at
90 Ω with its own skew budget, QSPI boot flash, and a 20-pin expansion header.

**Status: not fabricated, not powered, not routed here.** The board validates
clean and exports a complete KiCad project. Routed copper needs the
independent routing validator, which is not part of this build.

Two independent pairs on one board is what #40 case 3 asks about: each must
stay its own pair, on signal copper, with its own budgets.

## What this board answers, and what it does not

| #40 case 3 asks | This board |
| --- | --- |
| a differential interface declared | **Yes.** Two pairs, 90 Ω, `max_skew 0.15mm`, `couple tight`. |
| an inter-pair length budget | **Yes.** `match_length 0.5mm` on the `USB` class holds all four nets to one length. |
| the constraints preserved in KiCad | **No.** Neither budget survives export — see below. |
| routed evidence | **Not here.** No router in this build. |

## The budgets, and where they stop

```
netclass "USB" { trace_width 0.2mm  clearance 0.2mm  match_length 0.5mm }
diff_pair U2_usb_dp U2_usb_dn { impedance 90ohm  max_skew 0.15mm  couple tight }
diff_pair U4_usb_dp U4_usb_dn { impedance 90ohm  max_skew 0.15mm  couple tight }
```

`match_length` bounds the class's four nets against each other. `max_skew`
bounds one pair's own two halves, which `match_length` cannot express. A pair
usually needs both, and this board states both.

Neither reaches KiCad (#147). `clearance`, `trace_width`, `diff_pair_width`
and `diff_pair_gap` do reach the exported project file's net classes, which is
what makes the omission easy to miss — the net class looks like it carries the
design's constraints. A KiCad net class has no length or skew field; the
carrier for those is a `.kicad_dru` custom rule file, and Synth writes none.

Nor is either measured in this build: the check that sums a class's members
from emitted copper, and the one that compares a pair's halves, live in the
independent routing validator. The capability descriptor lists all three
attributes under `unverified`. A board may state a budget here and will not be
told when the copper misses it.

## Two substitutions, both forced

**The second endpoint is an RP2350, not the ESP32-C3 this design was first
written around.** `esp32_c3_mini` names a symbol and a footprint absent from
KiCad 10's libraries, and declares `dn`/`dp` on pins 20 and 21 — pins the
module does not have, since ESP32-C3's USB is GPIO18 and GPIO19, which the
same part already declares (#146). A board using it cannot export. What case 3
needs is two independent pairs, and two of a verified part give that.

**There are no keepouts.** The original carried one per pair, to keep other
copper out of each corridor. A declared keepout reaches no exported board
(#142), and even when it does, a KiCad keepout is unconditional — it would
forbid the pair it is meant to protect. Isolating a pair from other nets needs
a rule area with a net condition. Two `keepout` blocks claimed a constraint
the board did not have.

## Reproducing

```
synth validate qualification/class-c/dual-usb-compute/dual_usb_compute.synth
synth export-kicad qualification/class-c/dual-usb-compute/dual_usb_compute.synth --out <dir>
```

Clean validation and a complete project: schematic, board, symbol library, BOM
and pick-and-place.
