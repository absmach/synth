#!/usr/bin/env bash
#
# Install the external routing engines Synth drives.
#
# Synth does not generate copper itself: `synth export-kicad` / `synth route`
# hand the un-routed board to KiCadRoutingTools (the default) or FreeRouting
# and then validate what comes back. Neither engine is vendored, so a fresh
# checkout has to fetch them. This script does that, idempotently, without
# root: it sets up a KiCadRoutingTools checkout with a Python that can import
# KiCad's `pcbnew`, and downloads the pinned FreeRouting JAR.
#
# It prints the environment Synth needs at the end; nothing is written to your
# shell profile.
#
# Usage:
#   scripts/setup-routing-engines.sh [--freerouting-only|--krt-only] [--check]
#                                    [--krt-dir DIR] [--krt-python PYTHON]
#
set -euo pipefail

# The JAR the adapter searches for by name; bump both together.
FREEROUTING_VERSION="2.4.1"
FREEROUTING_URL="https://github.com/freerouting/freerouting/releases/download/v${FREEROUTING_VERSION}/freerouting-${FREEROUTING_VERSION}.jar"
KRT_REPO="https://github.com/drandyhaas/KiCadRoutingTools.git"
KRT_DEFAULT_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/synth/kicad-routing-tools"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JAR_DIR="${REPO_ROOT}/tools/freerouting"
JAR_PATH="${JAR_DIR}/freerouting-${FREEROUTING_VERSION}.jar"

DO_FREEROUTING=1
DO_KRT=1
CHECK_ONLY=0
KRT_DIR="${KRT_DEFAULT_DIR}"
KRT_PYTHON=""

say() { printf '%s\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --freerouting-only) DO_KRT=0 ;;
    --krt-only) DO_FREEROUTING=0 ;;
    --check) CHECK_ONLY=1 ;;
    --krt-dir) KRT_DIR="${2:?--krt-dir needs a path}"; shift ;;
    --krt-python) KRT_PYTHON="${2:?--krt-python needs a path}"; shift ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
  shift
done

# ── FreeRouting ──────────────────────────────────────────────────────────────

is_zip() {
  # A JAR is a zip; check the magic rather than trusting the extension, so a
  # truncated download or an HTML error page is caught here and not at run time.
  [ -f "$1" ] && [ "$(head -c 2 "$1" 2>/dev/null)" = "PK" ]
}

install_freerouting() {
  say "FreeRouting ${FREEROUTING_VERSION}"
  if is_zip "${JAR_PATH}"; then
    info "already installed: ${JAR_PATH}"
    return 0
  fi
  if [ "${CHECK_ONLY}" = 1 ]; then
    warn "not installed: ${JAR_PATH}"
    return 0
  fi
  mkdir -p "${JAR_DIR}"
  local tmp="${JAR_PATH}.partial"
  info "downloading ${FREEROUTING_URL}"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --retry 3 -o "${tmp}" "${FREEROUTING_URL}" || die "download failed"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "${tmp}" "${FREEROUTING_URL}" || die "download failed"
  else
    die "need curl or wget to download the FreeRouting JAR"
  fi
  is_zip "${tmp}" || die "downloaded file is not a JAR (bad URL or truncated download?)"
  mv "${tmp}" "${JAR_PATH}"
  info "installed ${JAR_PATH}"
  if ! command -v java >/dev/null 2>&1; then
    warn "no java on PATH; FreeRouting 2.4.1 needs a Java 25 runtime"
  else
    java_major="$(java -version 2>&1 | sed -n 's/.*version "\([0-9]*\).*/\1/p' | head -1)"
    if [ "${java_major:-0}" -lt 25 ]; then
      warn "FreeRouting 2.4.1 is Java 25 bytecode; java on PATH is ${java_major:-unknown}." \
           "Pass --freerouting-java <java25> or install a Java 25 JRE"
    fi
  fi
}

# ── KiCadRoutingTools ────────────────────────────────────────────────────────

# A Python that can import KiCad's board bindings. `pcbnew` ships with KiCad's
# own Python and is usually absent from the python3 first on PATH, which is the
# single most common setup failure.
find_pcbnew_python() {
  local candidate
  for candidate in ${KRT_PYTHON:+$KRT_PYTHON} \
                   "${SYNTH_FREEROUTING_PYTHON:-}" \
                   python3 python3.14 python3.13 python3.12 python \
                   /usr/bin/python3.14 /usr/bin/python3; do
    [ -n "${candidate}" ] || continue
    if "${candidate}" -c 'import pcbnew' >/dev/null 2>&1; then
      printf '%s\n' "$(command -v "${candidate}" 2>/dev/null || printf '%s' "${candidate}")"
      return 0
    fi
  done
  return 1
}

has_krt_deps() {
  "$1" -c 'import numpy, scipy, shapely, PIL' >/dev/null 2>&1
}

install_krt() {
  say "KiCadRoutingTools"
  if [ ! -d "${KRT_DIR}/.git" ]; then
    if [ "${CHECK_ONLY}" = 1 ]; then
      warn "no checkout at ${KRT_DIR}"
      return 0
    fi
    info "cloning into ${KRT_DIR}"
    mkdir -p "$(dirname "${KRT_DIR}")"
    git clone --depth 1 "${KRT_REPO}" "${KRT_DIR}" || die "clone failed"
  else
    info "checkout: ${KRT_DIR}"
  fi

  local python
  if ! python="$(find_pcbnew_python)"; then
    warn "no Python on PATH can import 'pcbnew'; install KiCad's Python or pass --krt-python"
    return 1
  fi
  info "python:   ${python} (imports pcbnew)"

  if ! has_krt_deps "${python}"; then
    if [ "${CHECK_ONLY}" = 1 ]; then
      warn "KRT python dependencies missing in ${python}"
    else
      # A virtualenv layered over the interpreter's site-packages keeps
      # `pcbnew` visible while installing numpy/scipy/shapely/Pillow somewhere
      # writable — no --break-system-packages and no root.
      local venv="${KRT_DIR}/.venv"
      info "creating venv ${venv} (--system-site-packages)"
      "${python}" -m venv --system-site-packages "${venv}" || die "venv creation failed"
      "${venv}/bin/pip" install --quiet --upgrade pip || true
      info "installing KRT requirements"
      "${venv}/bin/pip" install --quiet -r "${KRT_DIR}/requirements.txt" \
        || die "pip install failed"
      python="${venv}/bin/python"
    fi
  fi

  if [ "${CHECK_ONLY}" = 0 ]; then
    if [ -f "${KRT_DIR}/rust_router/grid_router.so" ]; then
      info "grid_router already built"
    else
      info "building the Rust router"
      "${python}" "${KRT_DIR}/build_router.py" || die "build_router.py failed"
    fi
  fi
}

# ── Report ───────────────────────────────────────────────────────────────────

report() {
  say ""
  say "Environment for Synth:"
  say "  export SYNTH_FREEROUTING_JAR=${JAR_PATH}"
  local python
  if python="$(find_pcbnew_python 2>/dev/null)"; then
    say "  export SYNTH_FREEROUTING_PYTHON=${python}"
  else
    say "  # SYNTH_FREEROUTING_PYTHON: no Python with pcbnew found yet"
  fi
  say "  export KICAD_ROUTING_TOOLS_REPO=${KRT_DIR}"
  say ""
  say "Then check what Synth can see:"
  say "  synth routers"
}

install_freerouting
[ "${DO_FREEROUTING}" = 1 ] || say "(skipping FreeRouting)"
if [ "${DO_KRT}" = 1 ]; then
  install_krt || true
else
  say "(skipping KiCadRoutingTools)"
fi
report
