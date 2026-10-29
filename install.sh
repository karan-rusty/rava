#!/bin/sh
# Install a prebuilt rava binary from GitHub releases.
#
#   curl -LsSf https://raw.githubusercontent.com/karan-rusty/rava/main/install.sh | sh
#   sh install.sh --version v0.1.0 --prefix /usr/local
#
# The prebuilt binaries are static musl builds for x86_64 and aarch64 Linux.
# Live audio capture needs PulseAudio or PipeWire (`parec`).

set -eu

REPO="karan-rusty/rava"
VERSION="latest"
PREFIX=""
UNINSTALL=0

usage() {
    cat <<'EOF'
Install rava from GitHub releases.

Usage: install.sh [OPTIONS]

Options:
  --version TAG   Release to install, with or without the "v" (default: latest)
  --prefix DIR    Install prefix; the binary goes in DIR/bin
                  (default: $HOME/.local)
  --uninstall     Remove the installed rava binary
  -h, --help      Show this help

Examples:
  curl -LsSf https://raw.githubusercontent.com/karan-rusty/rava/main/install.sh | sh
  sh install.sh --version v0.1.0 --prefix /usr/local
  sh install.sh --uninstall

The binary is installed into DIR/bin. If that directory is not on your PATH,
the installer prints the command to add it; it does not modify startup files.
EOF
}

die() {
    printf 'install.sh: %s\n' "$1" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || die "--version needs a value"
            VERSION=$2
            shift 2
            ;;
        --version=*)
            VERSION=${1#*=}
            shift
            ;;
        --prefix)
            [ $# -ge 2 ] || die "--prefix needs a value"
            PREFIX=$2
            shift 2
            ;;
        --prefix=*)
            PREFIX=${1#*=}
            shift
            ;;
        --uninstall)
            UNINSTALL=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1 (try --help)"
            ;;
    esac
done

if [ -z "$PREFIX" ]; then
    [ -n "${HOME:-}" ] || die "HOME is not set; pass --prefix DIR"
    PREFIX="$HOME/.local"
fi
case "$PREFIX" in
    */) PREFIX=${PREFIX%/} ;;
esac
[ -n "$PREFIX" ] || PREFIX=/
BIN_DIR="$PREFIX/bin"

if [ "$UNINSTALL" -eq 1 ]; then
    if [ -e "$BIN_DIR/rava" ]; then
        rm -f "$BIN_DIR/rava"
        printf 'removed %s\n' "$BIN_DIR/rava"
    else
        printf 'rava is not installed in %s\n' "$BIN_DIR"
    fi
    exit 0
fi

os=$(uname -s)
if [ "$os" != "Linux" ]; then
    die "prebuilt binaries are Linux-only (found $os); build from source with: cargo install --git https://github.com/$REPO --locked"
fi

case "$(uname -m)" in
    x86_64|amd64) target="x86_64-unknown-linux-musl" ;;
    aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
    *) die "unsupported architecture: $(uname -m) (prebuilt binaries exist for x86_64 and aarch64)" ;;
esac

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

if [ "$VERSION" = "latest" ] || [ -z "$VERSION" ]; then
    final_url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") \
        || die "could not find the latest release (does one exist yet?)"
    tag=${final_url##*/}
    case "$tag" in
        v[0-9]*) ;;
        *) die "could not find the latest release tag (got '$tag')" ;;
    esac
else
    case "$VERSION" in
        v[0-9]*) tag="$VERSION" ;;
        [0-9]*) tag="v$VERSION" ;;
        *) die "invalid version: $VERSION" ;;
    esac
fi

version=${tag#v}
archive="rava-${version}-${target}.tar.gz"
download_url="https://github.com/$REPO/releases/download/$tag"

tmp=$(mktemp -d) || die "could not create a temporary directory"
cleanup() {
    rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

printf 'downloading rava %s (%s)\n' "$version" "$target"
curl -fsSL --retry 3 -o "$tmp/$archive" "$download_url/$archive" \
    || die "download failed: $download_url/$archive"
curl -fsSL --retry 3 -o "$tmp/$archive.sha256" "$download_url/$archive.sha256" \
    || die "checksum download failed: $download_url/$archive.sha256"

(
    cd "$tmp"
    sha256sum -c "$archive.sha256"
) || die "checksum verification failed for $archive"

tar -xzf "$tmp/$archive" -C "$tmp" || die "could not extract $archive"
src="$tmp/rava-${version}-${target}/rava"
[ -f "$src" ] || die "archive $archive does not contain rava"

mkdir -p "$BIN_DIR" || die "could not create $BIN_DIR"
if command -v install >/dev/null 2>&1; then
    install -m 0755 "$src" "$BIN_DIR/rava" \
        || die "could not write $BIN_DIR/rava (try --prefix DIR)"
else
    cp "$src" "$BIN_DIR/rava" && chmod 0755 "$BIN_DIR/rava" \
        || die "could not write $BIN_DIR/rava (try --prefix DIR)"
fi

printf 'installed rava %s to %s\n' "$version" "$BIN_DIR/rava"

on_path=0
case ":${PATH:-}:" in
    *":$BIN_DIR:"*) on_path=1 ;;
esac

if [ "$on_path" -eq 1 ]; then
    resolved=$(command -v rava 2>/dev/null || true)
    if [ "$resolved" = "$BIN_DIR/rava" ]; then
        printf 'rava is on your PATH; open a new terminal and run `rava` from any directory.\n'
    else
        printf 'installed to %s, but `rava` currently resolves to %s.\n' \
            "$BIN_DIR/rava" "${resolved:-nothing}"
        printf 'Put %s earlier in your PATH.\n' "$BIN_DIR"
    fi
else
    shell=${SHELL:-}
    shell=${shell##*/}
    printf '\n%s is not in your PATH. Add it with:\n' "$BIN_DIR"
    case "$shell" in
        fish)
            printf '  fish_add_path "%s"\n' "$BIN_DIR"
            printf 'Run that in fish; it takes effect immediately and in new shells.\n'
            ;;
        bash)
            printf '  echo '\''export PATH="%s:$PATH"'\'' >> ~/.bashrc\n' "$BIN_DIR"
            printf 'Then open a new terminal.\n'
            ;;
        zsh)
            printf '  echo '\''export PATH="%s:$PATH"'\'' >> ~/.zshrc\n' "$BIN_DIR"
            printf 'Then open a new terminal.\n'
            ;;
        *)
            printf '  export PATH="%s:$PATH"\n' "$BIN_DIR"
            printf 'Add that line to your shell profile to keep it.\n'
            ;;
    esac
    printf '(The installer does not edit your shell startup files.)\n'
fi

if version_output=$("$BIN_DIR/rava" --version 2>/dev/null); then
    printf '%s\n' "$version_output"
else
    printf 'warning: the installed binary did not run; check the architecture\n' >&2
fi
