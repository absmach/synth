# Split ground planes: design decision

Status: accepted, 2026-11-13. Replaces the open question in
[#58](https://github.com/absmach/synth/issues/58).

## Decision

Synth treats analog and digital ground as **one net with declared domains**,
not as two unconditionally split planes.

1. A design names its ground domains in the source with ordinary nets
   (`net "AGND"`, `net "DGND"`) and joins them at exactly one declared point
   with a `net` tie, a net-tie part, or a 0 Ω link.
2. The exporter pours **each ground-like net on the copper the other ground
   nets do not use**. It does not give two ground nets the same plane and let
   them merge silently; that behaviour was the bug in #58.
3. A split is a placement-and-routing decision, not a syntax-level one.
   Synth does not invent a join point. If two ground domains are declared and
   no tie is declared, validation reports the domains as unjoined rather than
   guessing.
4. Return-path checks keep using the net class and the `edge_rate` tier. A
   domain boundary is not a reference plane; the check does not pretend it is.

## Why not split by default

A split plane only helps when the return current would otherwise cross a noisy
region, and it only helps if every signal that crosses the split has a
stitching path. On a two-layer board a split usually makes EMC worse: the
return current has no adjacent plane to follow, so it detours and radiates.
KiCad's own DRC cannot evaluate either effect. Making a split automatic would
therefore manufacture a claim Synth cannot verify.

## What Synth verifies

- Each declared ground net is a distinct net; two names are never merged by
  the exporter.
- A declared join exists and connects the domains. A design with declared
  domains and no join is a finding.
- Copper for one domain does not flood the other domain's plane layers.
- Connector, mounting-hole and keepout zones apply to every ground domain.

## What remains a human review

- Whether a split is appropriate for the stackup, the signalling and the
  enclosure.
- Where the single join point belongs.
- Whether every signal crossing the boundary has a stitching capacitor or via.

## Follow-ups

- A `ground_domain` block that declares a join and a stitching rule is worth
  adding once the checks above have a reference board to test against
  ([#464](https://github.com/absmach/synth/issues/464)).
- The Class B analog front-end board (#69) is the first board that should
  exercise a declared AGND/DGND join.
