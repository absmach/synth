#!/usr/bin/env bash
# reproduce.sh [work-dir]
#
# Rebuilds the protected power hub's board files from protected_power_entry.synth
# and the placement sidecar beside it, three times, and compares the three exports.
# Needs git, a Rust toolchain, Python 3.12 with venv, and KiCad 10.0.6's symbol and
# footprint libraries (KICAD_SYMBOL_DIR and KICAD_FOOTPRINT_DIR set) and kicad-cli
# 10.0.6 for the fabrication package. Each export takes about 8 minutes.
#
# The versions it uses are in pins.env. Synth is built from absmach/synth at the
# pinned commit, in upstream/main: the merge of Synth pull request #388, with
# #370 and #394 before it, which carry the silkscreen, via, label and
# return-via changes this board uses. The parts the design uses that are not in
# that checkout's registry are in user-registry/.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
board=$(cd "$here/.." && pwd)
work=${1:-$here/work}
# shellcheck source=pins.env
. "$here/pins.env"
: "${KICAD_SYMBOL_DIR:?set KICAD_SYMBOL_DIR to the KiCad $KICAD_VERSION symbol libraries}"
: "${KICAD_FOOTPRINT_DIR:?set KICAD_FOOTPRINT_DIR to the KiCad $KICAD_VERSION footprint libraries}"

kicad_now=$(kicad-cli version 2>/dev/null || true)
[ "$kicad_now" = "$KICAD_VERSION" ] || echo "warning: kicad-cli is '$kicad_now', the artifacts were made with $KICAD_VERSION" >&2

mkdir -p "$work"
[ -d "$work/synth/.git" ] || git clone --quiet "$SYNTH_REPO" "$work/synth"
git -C "$work/synth" fetch --quiet origin
git -C "$work/synth" checkout --quiet "$SYNTH_COMMIT"
(cd "$work/synth" && cargo build --release -p synth-cli)

[ -d "$work/krt/.git" ] || git clone --quiet "$KRT_REPO" "$work/krt"
git -C "$work/krt" checkout --quiet "$KRT_COMMIT"
[ -x "$work/venv/bin/python" ] || python3 -m venv "$work/venv"
# shellcheck disable=SC2086
"$work/venv/bin/pip" install --quiet $PY_PACKAGES

rm -rf "$work/src" "$work"/out1 "$work"/out2 "$work"/out3
mkdir -p "$work/src"
cp "$board/protected_power_entry.synth" "$board/protected_power_entry.placement.layout.toml" "$work/src/"
export SYNTH_USER_REGISTRY_DIR="$here/user-registry"
export SYNTH_ROUTER_TIMEOUT_SECS=900
for n in 1 2 3; do
  (cd "$work/src" && "$work/synth/target/release/synth" export-kicad protected_power_entry.synth \
    --out "$work/out$n" --router kicad-routing-tools \
    --kicad-routing-tools-repo "$work/krt" --kicad-routing-tools-python "$work/venv/bin/python")
done

python3 "$here/compare_exports.py" "$work"

# The fabrication package from the first export, and its hashes next to the committed ones.
bash "$board/agent-run/tools/fab_package.sh" "$work/out1" protected_power_entry "$work/fab" \
  "$board/agent-run/tools/sourcing.csv"
python3 "$board/agent-run/tools/gerber_content_hash.py" "$work/fab/protected_power_entry_gerbers.zip"
echo "committed: $(cat "$board/fab/GERBER_CONTENT_SHA256")"
