# W-SYNTH-REG-001 — User registry part shadows a shipped part

**Severity:** warning
**Stage:** registry load — tiered (load_tiered)

## What this means

The component registry is loaded in two tiers:

- **Tier 1 (shipped):** `registry/parts` — version-controlled, reviewed parts.
- **Tier 2 (user):** the directory returned by `user_registry_dir()` — i.e.
  `$SYNTH_USER_REGISTRY_DIR`, then `$XDG_DATA_HOME/synth/registry/parts`, then
  `~/.local/share/synth/registry/parts`.

When a Tier 2 part id collides with a Tier 1 part id, the Tier 2 part wins (user
overrides shipped), and `W-SYNTH-REG-001` is emitted so the operator knows a
shipped, reviewed part has been silently superseded by a local override.

This is advisory. It does **not** block compilation, but a shadowed shipped part
means the design is no longer using the reviewed definition — a common source of
"works on my machine" fabrication surprises.

## Minimal reproduction

```toml
# registry/parts/mcus/rp2350.synth.toml   (Tier 1, reviewed)
# ...
id = "rp2350"
```

```toml
# ~/.local/share/synth/registry/parts/mcus/rp2350.synth.toml   (Tier 2, local)
# ...
id = "rp2350"
```

```console
$ synth registry doctor
W-SYNTH-REG-001: user part `rp2350` shadows shipped (at /home/you/.local/share/synth/registry/parts/mcus/rp2350.synth.toml)
```

## Suggested fix

1. If the override is intentional and correct, document it and keep it — the
   warning is expected.
2. If the override is stale or accidental, delete the Tier 2 file (or rename the
   part id) so the shipped, reviewed definition is used again.
3. To inspect resolution, run `synth registry list` (with
   `SYNTH_USER_REGISTRY_DIR` set) and confirm which file backs each part id.

## Gating

- `W-SYNTH-REG-001` is emitted only by the **tiered** loader (`load_tiered`).
  The flat `load_dir` loader does not compare tiers and therefore never emits it.
- It is a `Severity::Warning`; `--strict-registry` does **not** escalate it to an
  error (shadowing is permitted by design).
