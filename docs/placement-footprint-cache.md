# Placement footprint cache

## Cause

`kicad_footprint_loader::read_footprint` opens, reads and parses the same
`.kicad_mod` on every call. The placement path reaches it on every rectangle
access:

```
State::rect
  -> sidecar_courtyard_rect
  -> pcb_courtyard_geometry_for_part
  -> kicad_footprint_loader::courtyard_rect
  -> read_footprint
```

`relax_pass` walks all `121 * 120 / 2 = 7_260` component pairs, calls `rect` on
both sides, and runs up to `RELAX_PASSES = 200` times. The group rule tries up
to twelve packings, and `apply` can run the fallback variants several times, so
one `synth place` performs tens of millions of reads of the same 121 files. The
review phases (`build_pad_offset_lookup`, `pad_kinds`,
`parts_holding_outline`, `exported_pad_bounds`) read the same files again.

## Two layers

1. **Parsed footprint values.** `kicad_footprint_loader` memoizes the pads and
   the courtyard rectangle per resolved footprint path. The values are derived
   only from the file, and a computed miss (`no pads`, `no courtyard`) is
   cached too, so a file without one is not re-read. A failed read is never
   cached.
2. **Per-placement courtyard geometry.** `synth-place`'s `State` stores
   `(centre offset, size)` for every placed part, computed when the `State` is
   built. `State::rect` only rotates and translates those numbers. The vector
   lives and dies with the `State`; it is never shared between placements or
   candidates.

Layer 1 removes the file I/O from both the relax loop and the review phases.
Layer 2 removes the per-call geometry lookup from the relax loop.

## Invalidation

- Bundled KiCad footprints, and anything resolved through
  `KICAD_FOOTPRINT_DIR`, are treated as immutable for the life of the process,
  so their entries need no key beyond the resolved path.
- Tier-2 user footprints (`SYNTH_USER_FOOTPRINT_DIR`) are keyed by resolved
  path plus file mtime. Editing one between two calls in the same process is
  seen on the next call. The key is mtime equality, so an edit that leaves the
  mtime unchanged is not seen.
- The registry reload points clear the cache: `load_registry_strict` in
  `crates/synth-cli/src/main.rs` (the commands that lower a board, including
  the second load after automatic footprint repair) and
  `load_registry_tiered` in `crates/synth-mcp/src/tools.rs` (every MCP tool
  call that resolves parts) call `kicad_footprint_loader::clear_cache()`.
  Registry inspection commands run in their own process with an empty cache.
- A missing or unreadable footprint is not cached: it returns `None`, leaves no
  map entry, and is retried, so a transient failure does not stick. A file
  that was missing and then appears is read on the next call. A successful
  parse that finds no pads or no courtyard is cached as `Absent`, because that
  is a property of the file, not a read failure.
- The user footprint directory itself is resolved once by `bundled_dir()`'s
  `OnceLock`, as it was before this cache. There is no in-process user-dir
  reload; changing `SYNTH_USER_FOOTPRINT_DIR` takes effect in a new process.
  Registry reloads clear the parsed values, and per-file mtime covers edits
  inside the directory the process resolved.
- A per-`State` geometry vector is invalidated with the `State`; it stores only
  values from the part file, never from a placement.

## Measurements

Board: `qualification/class-d/cm4-carrier-clean/cm4_carrier.synth` (121 parts,
no sidecar), registry in `registry/parts` (177 parts), `XDG_DATA_HOME` an empty
directory. The archive originally supplied for this task lived under `/tmp` and
was removed by a host restart; the board and registry were restored from the
archived qualification directory, with no sidecar added.

Binary: `cargo build --release -p synth-cli`.

Command:

```sh
synth place cm4_carrier.synth --registry ../../../registry/parts --pretty
```

The machine restarted twice while the task ran, so the true `upstream/main`
carrier run could not finish: it was stopped in `rule_groups` after 97 minutes.
Its stage trace before the restart is the "before" evidence below. The carrier
byte-identity reference is the same old placement algorithm with layer 1
active and `State::rect` still on its per-call path; that run takes 228.49 s and
is the algorithm the cache must not change. The three small boards compare the
true upstream binary with the final binary.

Carrier, medians of three clean after runs (load 10.8 to 11.9 at launch):

| Metric | Before | After |
| --- | ---: | ---: |
| `synth place` wall | did not finish; killed in `rule_groups` after 97 min | **11.59 s** (8.15 / 11.59 / 13.69) |
| `place_solve` | 129.4 s | 2.03 s |
| `rule_usb` | 288.1 s | 0.153 s |
| `rule_groups` (all calls) | still running after 97 min | 4.51 s |
| `apply_rules` | not reached | 4.86 s |
| `read` syscalls | 12.2 M at 18 min, still in `rule_groups` | **1,422** |
| Placement JSON | old algorithm: 228.49 s, sha256 `71d19133…` | identical, sha256 `71d19133…` |

The 12.2 M figure is a `/proc/<pid>/io` sample; the process was killed before
it finished. A previous profile of this board recorded 32.3 M read syscalls
before it too was stopped in `rule_groups`.

Small boards, before is the true upstream binary:

| Board | Before wall | After wall | Byte identity |
| --- | ---: | ---: | --- |
| `qualification/class-b/usb-uart/usb_uart.synth` | 8.37 s | 0.64 s | identical, sha256 `ad50954e…` |
| `qualification/class-b/buck/buck.synth` | 18.93 s | 3.37 s | identical, sha256 `25b59ab5…` |
| `examples/led/led.synth` | 0.70 s | 0.37 s | identical, sha256 `f28e4df5…` |

The carrier comparison is `cmp` on the saved JSON, not a regenerated golden.
The smaller layer-1-only intermediate (old `rect`, cached reads) takes 228.49 s,
which shows that layer 1 alone removes the file I/O but leaves the per-call
geometry lookup in the relax loop; layer 2 removes that.

## What is not cached and why

- The raw footprint text used by `inline_body` and
  `derive_footprint_dimensions`. Those are export and import paths, not the
  placement hot path, and each part is read once there.
- Failed reads, and values for a user footprint whose mtime cannot be read. A
  failure returns the same `None`/fallback as before and is retried.
- Anything derived from a placement, a candidate, an iteration order or a
  tie-break. None of it enters either cache.
- `courtyard_extent_from_text` is not separately keyed. `courtyard_rect` caches
  the extent result, so the extent of a file is computed once; there is no
  separate `courtyard_extent(lib_id)` entry point.
- The cache is one entry per resolved path (121 entries on this board),
  holding one `Vec<Pad>` and one courtyard tuple per file; it is cleared on
  registry reload.

## Review evidence

- Callers of `courtyard_rect` (`pcb_courtyard_geometry_for_part` in
  `crates/synth-layout/src/lib.rs` and the footprint-cover check in
  `crates/synth-place/src/lib.rs`) and of `pads` (`synth-drc`, `synth-place`,
  `synth-validate`, `synth-kicad`, `synth-layout`) all receive the same
  `Option`/`Vec` the old read produced.
  `parse_pads` is the old parsing body verbatim, including the empty result
  mapping to `None`. The automatic footprint repair checks `pads` before it
  writes, then reloads the registry, which calls `clear_cache`, so it never
  reads a pre-repair entry.
- The cache is a `Mutex<HashMap<PathBuf, CachedFootprint>>` in a `OnceLock`.
  It is `Sync`; both lock sites recover a poisoned lock with
  `PoisonError::into_inner`; neither `parse_pads` nor `courtyard_rect_from_text`
  calls back into the cache, so there is no re-entrant lock and no deadlock.
  The lock is held across a first read and parse, which serialises those first
  misses; later calls only clone the cached value.
- `State::placed` is only ever assigned a clone of itself or the placement
  slice it was built from, and only `.center` and `.rotation` are mutated in
  place. No code sorts, pushes, removes or reorders `placed`, so
  `geometry[index]` stays aligned with `placed[index]`.
- A failed read leaves no entry, so a missing footprint is retried and does
  not grow the map. A bundled file deleted after a successful read still
  answers from the cache; that is the stated "bundled files do not change
  in-process" assumption. A user file deleted after a successful read is
  noticed by the mtime/stat check and returns `None` as before.
- Memory is one `PathBuf` and one small struct per successfully read path,
  about 0.25 MB for this board's 121 parts, cleared on registry reload.

## Verification

```
cargo test -p synth-place --test snapshots
  10 passed, 0 failed (no golden regenerated)

cargo test -p synth-place -p synth-kicad -p synth-layout --no-fail-fast
  synth-kicad: 438 passed, 2 failed, 2 ignored
  synth-layout: 393 passed, 0 failed
  synth-place: 329 passed, 0 failed, 14 ignored
  total: 1160 passed, 2 failed, 16 ignored

failures in that full run:
  synth-kicad --test golden_svg
    reference_schematics_match_golden_fingerprints (known local-only)
  synth-kicad --test erc_golden
    all_reference_designs_pass_kicad_erc (kicad-cli ERC hit its time budget
    under load; passed on isolated rerun: 1 passed, 86.42 s)
  (schematic_paper every_standard_paper_reaches_the_exported_schematic passed
  in this environment)

cargo fmt --check: pass
cargo clippy -p synth-place -p synth-layout -p synth-kicad \
  --all-targets -- -D warnings: exit 0
```

Local tests added:

- `kicad_footprint_loader::tests::a_bundled_footprint_is_read_once`: the file
  is deleted between two calls and the second `courtyard_rect` and `pads`
  calls still answer from the cache.
- `kicad_footprint_loader::tests::a_missing_footprint_is_retried`: a missing
  file returns `None` and is read after it appears, so a failed read is not
  cached.
- `kicad_footprint_loader::tests::a_user_footprint_edit_is_seen`: a user
  footprint is rewritten with a new mtime between two calls and the second
  answer changes.
- `default_rules::tests::cached_courtyard_geometry_matches_the_per_call_rect_for_every_rotation`:
  every component of `group_corner_hole` and `usb_diff_pair`, at all four
  rotations, matches `sidecar_courtyard_rect`.
