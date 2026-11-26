# E-SYNTH-POWER-015 — feedback divider margin is unproven

**Severity:** warning
**Stage:** erc — power

## What this means

A regulator has a two-resistor feedback divider, but the worst-case
analysis ([`E-SYNTH-POWER-013`](E-SYNTH-POWER-013.md)) cannot run because an
input is missing. Synth does not assume a tolerance, a reference range or a
load window, so the output margin is not checked. This is incomplete
evidence, not a pass.

The message lists each missing input:

- the regulator part's `feedback_vref_min_v` / `feedback_vref_max_v`;
- a resistance on either resistor that reads as one (`value "10k"`); a value
  such as `"DNP"` or a typo leaves the divider unanalysed;
- a tolerance on either resistor (`tolerance "1%"`, `value "10k 1%"`, or a
  chip-resistor MPN that encodes one);
- a supply window on a load of the output net (`voltage_min_v` /
  `voltage_max_v` on the load pin, or the load part's
  `operating_conditions`).

## Suggested fix

Add the missing data. Take reference ranges from the regulator data sheet's
electrical characteristics table, and cite the table in a comment in the
part file. Never copy a typical value into both bounds.

## Registry fields

All are optional keys under a regulator part's `[operating_conditions]`, read
by this check and by [`E-SYNTH-POWER-013`](E-SYNTH-POWER-013.md). Only the
reference range is required; the others widen the worst case when present
and count as zero when absent.

| Key | Unit | Data sheet source | Effect |
|---|---|---|---|
| `feedback_vref_min_v` | V | feedback/reference voltage, minimum over temperature | lower bound of the reference |
| `feedback_vref_max_v` | V | the same row, maximum | upper bound of the reference |
| `feedback_leakage_max_a` | A | feedback pin current (`IFB`), maximum | shifts the output by up to `IFB x Rtop` either way |
| `load_regulation_pct` | % of Vout | load regulation over the rated load range | widens the output window by this much |
| `line_regulation_pct` | % of Vout | line regulation over the input range | widens the output window by this much |

Example, from `lmr33640.synth.toml`:

```toml
[operating_conditions]
# LMR33640 datasheet SNVSB99C, 7.5 Electrical Characteristics: VFB 0.985 / 1 / 1.015 V (ADJ), IFB 50 nA max.
feedback_vref_min_v = 0.985
feedback_vref_max_v = 1.015
feedback_leakage_max_a = 50e-9
```

A regulator without a reference range, such as `mp2307_buck` today, keeps
raising this warning on every divider until the range is added from its data
sheet.
