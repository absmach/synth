#!/usr/bin/env bash
#
# Install kicad-cli and the stock symbol/footprint libraries from the KiCad
# 10.0 PPA, and refuse to continue with anything else.
#
# Why this is a script and not four inline `apt-get install` lines: the
# obvious spelling is silently wrong. `apt-get update` exits 0 when a
# repository index fails to download — it prints `W: Failed to fetch ... 503`
# and carries on with the indexes it already has. Ubuntu noble's universe
# component also ships `kicad`, at 7.0.11. So a PPA outage does not fail the
# install step; it installs KiCad 7 and lets the build continue.
#
# That is not a version skew anyone notices in the log. KiCad 7 numbers the
# USB-C receptacle shield `S1` and KiCad 10 numbers it `SH`, so the registry's
# strict pin check rejects `usb_c_receptacle` and every board that loads the
# registry panics, which reads as a board defect in whichever pull request
# happened to be running. It cost one of those a full review cycle.
#
# So the version is asserted twice: once on the candidate, before anything is
# installed, and once on the binary that ends up on PATH.
set -euo pipefail

want_major_minor="10.0"

sudo add-apt-repository --yes ppa:kicad/kicad-10.0-releases

# Transient 503s from launchpadcontent.net are common enough to be worth a
# retry rather than a failed run.
sudo apt-get update -o Acquire::Retries=5

# The gate. `apt-cache policy` reports the version apt would install, so this
# catches the archive fallback before it happens rather than after.
candidate="$(apt-cache policy kicad | awk '/Candidate:/ { print $2 }')"
case "$candidate" in
  "$want_major_minor".*) ;;
  *)
    echo "::error::the KiCad ${want_major_minor} PPA is unusable: apt would install kicad ${candidate:-<none>}."
    echo "::error::refusing to fall back to the Ubuntu archive, whose stock libraries number pins differently."
    apt-cache policy kicad
    exit 1
    ;;
esac

sudo apt-get install --no-install-recommends -y \
  kicad kicad-symbols kicad-footprints

# And again on the installed binary, because the candidate check proves what
# apt intended rather than what PATH now resolves to.
installed="$(kicad-cli version)"
case "$installed" in
  "$want_major_minor".*) ;;
  *)
    echo "::error::expected kicad-cli ${want_major_minor}.x, got ${installed}."
    exit 1
    ;;
esac
echo "kicad-cli ${installed}"

# The library tables the exporters read. `kicad-cli` resolves `Connector:...`
# through these, and without them a stock symbol is not findable at all.
mkdir -p "$HOME/.config/kicad/${want_major_minor}"
cp /usr/share/kicad/template/sym-lib-table "$HOME/.config/kicad/${want_major_minor}/sym-lib-table"
cp /usr/share/kicad/template/fp-lib-table "$HOME/.config/kicad/${want_major_minor}/fp-lib-table"

# Tests that need a stock footprint library read it from here, the directory the
# libraries were just checked in; later steps of the job inherit it.
if [ -n "${GITHUB_ENV:-}" ]; then
  echo "KICAD_FOOTPRINT_DIR=${footprints_dir}" >> "$GITHUB_ENV"
fi
