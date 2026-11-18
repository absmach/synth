# Release checks

For CI and agent workflows, `synth check DESIGN.synth --json` is the single deterministic, model-free gate for compiler validation and DRC. Add `--fab` to require a complete KiCad manufacturing export and receive SHA-256 hashes for the emitted files. A timed-out or unavailable physical stage is reported as `unknown` and blocks the command; it is never treated as a pass.

## Tri-state native stages

Stages that shell out to `kicad-cli` are tri-state, because a check that could not run is neither a pass nor a failure:

| Status    | Meaning                                                                                                                           |
| --------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `pass`    | The tool ran and reported no blocking violations.                                                                                 |
| `fail`    | The tool ran and rejected the design.                                                                                             |
| `unknown` | No evidence. Missing or unsupported `kicad-cli`, a missing symbol/footprint library, a crash, a timeout, or an unreadable report. |

`unknown` blocks exactly as `fail` does, and `--force` does not override it — it overrides Synth's own findings, not the absence of verification. Likewise, unconnected pads and KiCad DRC errors block a release export, and `--force` does not override that. Each `unknown` reports the tool, its version, the exact command, its stderr, and a machine-stable reason (`not_installed`, `timeout`, `command_failed`, `report_malformed`, `unsupported_version`, ...). `synth check --fab` embeds that evidence under `stages.manufacturing.native`; `synth export-kicad --verification-report FILE` writes it directly.

A plain `synth export-kicad` still works with no KiCad installed: it records the `unknown` and sets `release_ready: false`, but does not block, since nothing about that output claims to have been verified. Requesting verification — `--validate-erc`, or any of `--gerbers`/`--drill`/`--step` — makes an `unknown` blocking. Set `KICAD_CLI` to point at a specific build, and `SYNTH_KICAD_CLI_TIMEOUT_SECS` to change the per-run budget (default 300s).

## Export overrides

`--force` and `--allow-unverified-parts` let development proceed past findings that would otherwise stop an export. Every use is recorded in `<out>/release.json` with the exact flag, the diagnostic it suppressed, the affected parts, and the reviewer state of every part on the board:

| Field               | Meaning                                                             |
| ------------------- | ------------------------------------------------------------------- |
| `release_ready`     | `false` whenever any override was used. Derived, not settable.      |
| `production_status` | `production` or `untrusted`.                                        |
| `overrides[]`       | flag, code, affected parts, and the consequence in plain words.     |
| `reviewer_state[]`  | per part: `reviewed_by`, `reviewed_at`, and whether it is reviewed. |

Human output on these paths leads with `UNTRUSTED / NOT FOR FABRICATION`.

`synth check --fab` reads that manifest and refuses a package carrying overrides. To proceed, record who authorized it and why:

```bash
synth check DESIGN.synth --fab \
  --override-exception "prototype run, not customer hardware" \
  --authorized-by "a release manager"
```

An exception authorizes the run; it does not clean the package. `release_ready` stays `false` and `production_status` stays `untrusted`, because an authorization is a decision about risk rather than evidence the risk is gone.

For CI and release gates, `synth export-kicad --safe` is the mode with no override path: it refuses to sit beside `--force` or `--allow-unverified-parts`, and then checks that the package it produced is actually clean.
