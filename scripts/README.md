# Synth Scripts Directory

This directory contains developer tooling for fixtures, registry maintenance, and reference exports.

---

## Anomaly Detector Training (`train_anomaly_model.py`)

Trains the **One-Class SVM Graph Anomaly Detector** (`W-SYNTH-ANOMALY-001`) used by `synth-validate`.

### Overview
- **Input:** Clean SynthSpec design fixtures (`pass__*.synth` under `fixtures/erc/` and passing designs under `fixtures/designs/`).
- **Feature Extraction:** 15 graph-level structural metrics (component count, net degree, passive ratio, power pin ratio, decoupling density, protocol pin counts, etc.) extracted via `dump_features.rs`.
- **Model:** `StandardScaler` + `OneClassSVM(kernel='rbf', nu=0.05, gamma=0.001)` from `scikit-learn`.
- **Output:** `crates/synth-validate/src/anomaly_model.json` (embedded at compile-time in Rust via `include_str!`).

### Running the Trainer
```bash
python3 scripts/train_anomaly_model.py
```

### Re-Training Cadence & Invariants
> [!IMPORTANT]
> **When to Re-Train:**
> 1. **New Fixtures Added:** Whenever new clean reference designs are added to `fixtures/designs/` or `fixtures/erc/pass__*.synth`.
> 2. **Registry Expansion:** When major component categories or capabilities are added to `registry/parts/`.
> 3. **Larger design coverage:** When production-scale designs are added to the codebase.

> [!NOTE]
> **Verification Gate:** The script enforces that the held-out false-positive rate (FPR) on 10 held-out clean fixtures is **< 5.0%**. If a re-training attempt fails this gate, the script aborts without modifying `anomaly_model.json`.
