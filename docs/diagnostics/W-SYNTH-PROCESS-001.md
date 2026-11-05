# W-SYNTH-PROCESS-001 — the declared manufacturer is not a known profile

**Severity:** warning
**Stage:** erc

## What this means

The board's `manufacturer` matches no built-in profile, so Synth judges it
against `jlc-standard`, the most conservative floor. Nothing else reports the
fallback: the router and the exporter take the standard floor and carry on.

The usual cause is a misspelt tier. `jlcpcb_advanced` is not `jlc-advanced`, so
a six-layer board that meant to buy the 0.2 mm drill and via-in-pad is held to
a 0.3 mm drill with none, and a ball-grid part's fanout fails later, in the
router, with no pointer back to the name.

## Example

    board "node" {
      layers 6
      manufacturer "jlcpcb_advanced"   // not a profile name
      ...
    }

## How to fix it

Use a profile name: `jlc-standard`, `jlc-filled-via`, `jlc-advanced`,
`pcbway` or `oshpark`. A vendor written loosely (`jlcpcb`) is accepted and
takes the standard tier.

## What this does not report

A recognised name, however loosely the vendor is written. It does not judge
the geometry against the profile; that is `E-SYNTH-PROCESS-003` and the DRC
stage.
