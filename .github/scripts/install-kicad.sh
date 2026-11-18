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
# Asserting this on `kicad` alone is not enough. The pin numbering lives in
# `kicad-symbols`, not in the binary, and apt resolves each package against
# whichever index it has: a partially-fetched PPA can leave `kicad` and
# `kicad-cli` on 10.0 while `kicad-symbols` and `kicad-footprints` still come
# from the archive. The binary would report 10.0, the gate would pass, and the
# registry pin check would still see KiCad 7 symbols -- the exact failure this
# script exists to stop, reached by a different route.
#
# So every package is asserted on its candidate before anything is installed
# and on its installed version afterwards, and then the library content itself
# is checked, because a version string is a claim about the package and the
# pin number is the thing that actually breaks.
set -euo pipefail

want_major_minor="10.0"

# Every package whose version decides whether the stock libraries agree with
# the registry.
packages=(kicad kicad-symbols kicad-footprints)

# apt keeps its package index and downloads in root-owned directories by
# default, which a CI cache step cannot restore or save without sudo. Point
# both at a user-owned cache so `actions/cache` can carry them between runs.
#
# This is a reliability fix, not a micro-optimisation: on 2026-11-06 one shard
# spent 43 minutes in this step because GitHub's Azure archive mirror served
# the ~130 MB dependency set at ~50 kB/s. A warm cache turns that download into
# an offline unpack; a cold one still hits the mirror, but the timeouts below
# keep a throttled mirror from holding the whole job.
apt_cache="${SYNTH_APT_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/synth-apt}"
mkdir -p "${apt_cache}/lists/partial" "${apt_cache}/archives/partial"

# apt normally drops to the _apt user to download; the cache lives under
# $HOME, which _apt cannot write to. apt itself still runs through sudo.
apt_cache_opts=(
  -o "Dir::State::lists=${apt_cache}/lists"
  -o "Dir::Cache::archives=${apt_cache}/archives"
  -o "APT::Keep-Downloaded-Packages=true"
  -o "APT::Sandbox::User=root"
  # The runner is ephemeral, so skipping dpkg's fsync costs nothing and shaves
  # real time off unpacking KiCad's ~200-package dependency tree.
  -o "DPkg::Options::=--force-unsafe-io"
)

# A stalled mirror used to hold a shard for 43 minutes. Cap each request and
# retry, so a bad mirror fails over instead of hanging the job.
apt_net_opts=(
  -o "Acquire::Retries=5"
  -o "Acquire::http::Timeout=30"
  -o "Acquire::https::Timeout=30"
)

die() {
  echo "::error::$1"
  shift
  for line in "$@"; do echo "::error::$line"; done
  exit 1
}

# The cache step runs as the runner user, but apt (via sudo) creates root-owned
# files in it, including 0700 `partial` directories. Hand them back on every
# exit path so the post-job save can read the cache.
restore_cache_ownership() {
  sudo chown -R "$(id -u):$(id -g)" "$apt_cache" 2>/dev/null || true
}
trap restore_cache_ownership EXIT

# add-apt-repository runs its own `apt-get update`; --no-update leaves the
# single, cache-aware update below as the only one.
sudo add-apt-repository --yes --no-update ppa:kicad/kicad-10.0-releases

# Transient 503s from launchpadcontent.net are common enough to be worth a
# retry rather than a failed run. A cache hit leaves the index valid, so a
# mirror outage that defeats the retries does not have to fail the job: the
# candidate gate below still refuses anything that is not KiCad 10.0.
sudo apt-get "${apt_cache_opts[@]}" "${apt_net_opts[@]}" update \
  || echo "::warning::apt-get update failed; continuing with the cached package index"

# The gate. `apt-cache policy` reports the version apt would install, so this
# catches the archive fallback before it happens rather than after -- and it
# is asked about each package separately, because they can diverge.
for pkg in "${packages[@]}"; do
  candidate="$(apt-cache "${apt_cache_opts[@]}" policy "$pkg" | awk '/Candidate:/ { print $2 }')"
  case "$candidate" in
    "$want_major_minor".*) echo "candidate ${pkg} ${candidate}" ;;
    *)
      apt-cache "${apt_cache_opts[@]}" policy "$pkg"
      die "the KiCad ${want_major_minor} PPA is unusable on Ubuntu $(. /etc/os-release && echo "${VERSION_ID} (${VERSION_CODENAME})"): apt would install ${pkg} ${candidate:-<none>}." \
          "refusing to fall back to the Ubuntu archive, whose stock libraries number pins differently." \
          "if the runner image moved to a release the PPA does not build for, pin runs-on to one it does."
      ;;
  esac
done

sudo apt-get "${apt_cache_opts[@]}" "${apt_net_opts[@]}" install \
  --no-install-recommends -y "${packages[@]}"

# And again on what was installed, because the candidate check proves what apt
# intended rather than what ended up on disk.
for pkg in "${packages[@]}"; do
  installed="$(dpkg-query --show --showformat='${Version}' "$pkg")"
  case "$installed" in
    "$want_major_minor".*) echo "installed ${pkg} ${installed}" ;;
    *) die "expected ${pkg} ${want_major_minor}.x, got ${installed}." ;;
  esac
done

# The binary on PATH, which is a different question again from the package.
installed="$(kicad-cli version)"
case "$installed" in
  "$want_major_minor".*) ;;
  *) die "expected kicad-cli ${want_major_minor}.x, got ${installed}." ;;
esac
echo "kicad-cli ${installed}"

# Finally the libraries themselves. A version string is a claim about a
# package; this is the pin the registry actually reads. `USB_C_Receptacle` is
# the canary because it is where the skew first showed up: KiCad 7 numbers the
# shield `S1`, KiCad 10 numbers it `SH`, and `registry/parts/connectors/
# usb_c_receptacle.synth.toml` declares `SH`. If this symbol disagrees, every
# board that loads the registry panics.
symbols_dir=/usr/share/kicad/symbols
footprints_dir=/usr/share/kicad/footprints
canary="${symbols_dir}/Connector.kicad_sym"
[ -r "$canary" ] || die "no stock symbol library at ${canary}."

shield="$(awk '
  /^\t\(symbol "USB_C_Receptacle"/ { inside = 1 }
  inside && /\(number "/ {
    match($0, /\(number "[^"]*"/)
    num = substr($0, RSTART + 9, RLENGTH - 10)
    if (num == "SH" || num == "S1") { print num; exit }
  }
  inside && /^\t\)/ { exit }
' "$canary")"
case "$shield" in
  SH) echo "stock symbols number the USB-C shield SH, as the registry expects" ;;
  "") die "could not find the USB_C_Receptacle shield pin in ${canary}." \
          "the stock symbol library is not the one this build was written against." ;;
  *)  die "stock symbols number the USB-C shield ${shield}, not SH." \
          "that is the KiCad 7 numbering: the libraries came from the Ubuntu archive." ;;
esac

[ -d "${footprints_dir}/Connector_USB.pretty" ] \
  || die "no stock footprint library at ${footprints_dir}/Connector_USB.pretty."
echo "stock libraries: $(ls -1 "$symbols_dir" | wc -l) symbol, $(ls -1d "${footprints_dir}"/*.pretty | wc -l) footprint"

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
