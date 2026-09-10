# W-SYNTH-ANOMALY-001 — Board graph statistical anomaly detected

**Severity:** warning
**Stage:** erc — anomaly

## What this means

A trained One-Class Support Vector Machine (One-Class SVM) evaluated 15 structural graph features of the board (component density, passive ratios, decoupling density, net degree distributions, power pin ratios, protocol pin distributions, and layer counts) and determined that the design topology lies outside the decision boundary ($S < 0.0$) of canonical passing baseline designs.

This diagnostic is advisory-only and does **not** block compilation or override deterministic ERC rules. It highlights structurally unusual designs that merit manual electrical engineering review.

## Minimal reproduction

```synth
board "anomaly_repro" {
  layers 1

  component U1: mcu "rp2350"
  component U2: mcu "rp2350"
  component U3: mcu "rp2350"
  component U4: mcu "rp2350"
  component U5: mcu "rp2350"

  // 5 MCUs wired in parallel with zero decoupling, zero passives, zero power sources
  connect U1.vdd_io -> U2.vdd_io
  connect U2.vdd_io -> U3.vdd_io
  connect U3.vdd_io -> U4.vdd_io
  connect U4.vdd_io -> U5.vdd_io
}
```

## Suggested fix

Review the design for:
1. Missing support passive components (decoupling capacitors, pull-up/pull-down resistors).
2. Unusual net fanout or high-degree net concentrations.
3. Missing power distribution rails or floating required pin structures.

## Long-term Governance & Model Re-Training

- **Agent Interaction:** `W-SYNTH-ANOMALY-001` is classified as `Severity::Warning`. In automated repair loops (such as the reference agent harness), warnings are advisory and do not stall or block the loop.
- **Model Re-Training:** The offline model artifact (`crates/synth-validate/src/anomaly_model.json`) is re-trained using `python3 scripts/train_anomaly_model.py` whenever the golden design fixture corpus is updated. The re-training script enforces a strict $<5.0\%$ False Positive Rate (FPR) gate on held-out clean designs.
