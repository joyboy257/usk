#!/bin/sh
# usk installer — POSIX-sh compatible.
#
# Modeled on `rustup`'s install script:
#   curl -fsSL https://sh.rustup.rs | sh
#
# Usage:
#   curl -fsSL https://usk.dev/install.sh | sh
#   USK_VERSION=v0.1.0 curl -fsSL https://usk.dev/install.sh | sh
#
# What it does:
#   1. Detects the host OS (linux/darwin) and CPU arch (x86_64/aarch64).
#   2. Downloads the matching release tarball from GitHub.
#   3. Verifies the SHA256 against a sidecar `.sha256` file in the same
#      release. (Skipped only if the sidecar can't be fetched — see
#      USK_SKIP_VERIFY below.)
#   4. Untars and moves the `usk` binary to `~/.local/bin/usk`,
#      creating the directory if it doesn't exist.
#   5. Prints next-step instructions.
#
# Environment variables:
#   USK_VERSION       Tag to install (default: latest). Example: v0.1.0
#   USK_INSTALL_DIR   Override the install directory (default: ~/.local/bin)
#   USK_GH_OWNER      GitHub owner/repo override (default: usk-rs/usk).
#                     NOTE: `usk-rs` is a placeholder. Update this when
#                     the project has a real GitHub home.
#   USK_SKIP_VERIFY   If non-empty, skip the SHA256 check.

set -u

# ---- constants & defaults ----------------------------------------

# Default version. We try to read the latest tag from the GitHub
# API; if that fails we fall back to this. The fallback is updated
# at release time.
DEFAULT_VERSION="v0.1.0"

# Default GitHub owner/repo. PLACEHOLDER — update when the project
# has a real GitHub home. The release workflow must produce
# artifacts named `usk-<os>-<arch>.tar.gz` at the same tag.
GH_OWNER_REPO="${USK_GH_OWNER:-usk-rs/usk}"

# Default install location. XDG-style: ~/.local/bin is on PATH
# for most desktop Linux distros. ~/.local/bin also works on macOS
# for users who add it to their shell rc.
INSTALL_DIR="${USK_INSTALL_DIR:-$HOME/.local/bin}"

# ---- helpers -----------------------------------------------------

log() {
    printf 'usk-install: %s\n' "$1"
}

err() {
    printf 'usk-install: error: %s\n' "$1" >&2
    exit 1
}

# Detect the host's uname in lowercase, restricted to the values we
# actually support.
detect_os() {
    os="$(uname -s 2>/dev/null)"
    case "$os" in
        Linux)  echo "linux" ;;
        Darwin) echo "darwin" ;;
        *)
            err "unsupported operating system: $os (this installer supports linux and darwin)"
            ;;
    esac
}

# Detect the host's CPU arch. We map every variant we know about
# to one of the two release artifact names.
detect_arch() {
    arch="$(uname -m 2>/dev/null)"
    case "$arch" in
        x86_64|amd64)        echo "x86_64" ;;
        aarch64|arm64)       echo "aarch64" ;;
        *)
            err "unsupported CPU architecture: $arch (this installer supports x86_64 and aarch64)"
            ;;
    esac
}

# Compute the SHA256 of a file in hex. We use `sha256sum` (Linux)
# or `shasum -a 256` (macOS) — both are POSIX-portable shell
# utilities.
sha256_file() {
    file="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        err "neither sha256sum nor shasum is available; cannot verify checksum"
    fi
}

# Fetch a URL to stdout. Tries curl first, then wget. We never use
# either's progress UI in non-TTY contexts so the script is safe to
# pipe from curl itself.
fetch() {
    url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$url"
    else
        err "neither curl nor wget is available; cannot download $url"
    fi
}

# Resolve the version to install. If USK_VERSION is unset, query
# the GitHub API for the latest release tag.
resolve_version() {
    if [ -n "${USK_VERSION:-}" ]; then
        echo "$USK_VERSION"
        return
    fi
    # GitHub returns the tag_name in the JSON body. We use a simple
    # grep rather than jq to keep the script dependency-free.
    body="$(fetch "https://api.github.com/repos/${GH_OWNER_REPO}/releases/latest" || true)"
    if [ -n "$body" ]; then
        tag="$(printf '%s' "$body" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed 's/.*"\([^"]*\)"$/\1/')"
        if [ -n "$tag" ]; then
            echo "$tag"
            return
        fi
    fi
    echo "$DEFAULT_VERSION"
}

# ---- main --------------------------------------------------------

os="$(detect_os)"
arch="$(detect_arch)"
version="$(resolve_version)"

artifact="usk-${os}-${arch}.tar.gz"
base_url="https://github.com/${GH_OWNER_REPO}/releases/download/${version}"
archive_url="${base_url}/${artifact}"
sha_url="${base_url}/${artifact}.sha256"

log "installing usk ${version} (${os}/${arch})"

# Work in a tempdir so we don't litter the user's cwd.
tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t usk-install)" || err "failed to create tempdir"
trap 'rm -rf "$tmpdir"' EXIT INT TERM HUP

archive_path="${tmpdir}/${artifact}"
sha_path="${tmpdir}/${artifact}.sha256"

log "downloading ${archive_url}"
fetch "$archive_url" >"$archive_path" || err "download failed (is ${version} published with an artifact for ${os}/${arch}?)"

if [ -z "${USK_SKIP_VERIFY:-}" ]; then
    log "verifying SHA256"
    if fetch "$sha_url" >"$sha_path" 2>/dev/null; then
        expected="$(awk '{print $1}' "$sha_path")"
        actual="$(sha256_file "$archive_path")"
        if [ "$expected" != "$actual" ]; then
            err "sha256 mismatch: expected $expected, got $actual"
        fi
        log "checksum ok"
    else
        log "warning: could not fetch ${sha_url}; skipping verification"
        log "  (set USK_SKIP_VERIFY=1 to silence this message)"
    fi
else
    log "skipping checksum verification (USK_SKIP_VERIFY is set)"
fi

# Extract. We honor a tarball that's either uncompressed (.tar) or
# gzipped (.tar.gz). POSIX tar on macOS auto-detects gzip, so the
# same `tar -xf` works for both.
tar -xf "$archive_path" -C "$tmpdir" || err "failed to extract archive"

# The archive is expected to contain a single `usk` binary at its
# root. Validate that the file exists and is a regular file.
binary="${tmpdir}/usk"
if [ ! -f "$binary" ]; then
    err "extracted archive did not contain an 'usk' binary at its root"
fi

# Make sure the install dir exists, then move the binary in.
if [ ! -d "$INSTALL_DIR" ]; then
    log "creating ${INSTALL_DIR}"
    mkdir -p "$INSTALL_DIR" || err "failed to create ${INSTALL_DIR}"
fi

# If there's an existing binary, try to remove it first. We use a
# plain `rm`; if the user has a read-only install dir (e.g. /usr/bin),
# they'll see a clear error.
if [ -e "${INSTALL_DIR}/usk" ] || [ -L "${INSTALL_DIR}/usk" ]; then
    rm -f "${INSTALL_DIR}/usk" || err "failed to remove existing ${INSTALL_DIR}/usk"
fi

mv "$binary" "${INSTALL_DIR}/usk" || err "failed to install to ${INSTALL_DIR}/usk"
chmod +x "${INSTALL_DIR}/usk" || err "failed to chmod +x ${INSTALL_DIR}/usk"

log "installed: ${INSTALL_DIR}/usk"
log "version:   $(usk --version 2>/dev/null || echo 'unknown')"

# PATH nudge.
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        log ""
        log "${INSTALL_DIR} is not on your PATH."
        # Single-quoted strings can't contain single quotes in POSIX sh,
        # so we build the export line without escaping $HOME/$PATH.
        shell_name="$(basename "${SHELL:-sh}")"
        log "Add it with:  echo export PATH=\$HOME/.local/bin:\$PATH >> ~/.${shell_name}rc"
        log "Or run:       ${INSTALL_DIR}/usk --help"
        ;;
esac

log ""
log "next: try it out:"
log "    usk --help"
log "    usk install ./path/to/skill --harness claude-code"
log ""
log "usk is plug-and-play: no registry server required for local installs."
