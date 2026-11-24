#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install-macos.sh [--prefix ABSOLUTE_DIRECTORY] [--dry-run] [--help]

Build and install larp and the unsigned LarpCC settings app on macOS.
The binary defaults to ~/.local/bin/larp. The app goes to ~/Applications/LarpCC.app.
Use --dry-run to print the plan without building or installing.
See the README for manual installation and uninstall instructions.
EOF
}

fail() {
  printf '%s\n' "$1" >&2
  exit "${2:-1}"
}

prefix=''
dry_run=false
while (($#)); do
  case "$1" in
    --help) usage; exit 0 ;;
    --dry-run) dry_run=true; shift ;;
    --prefix)
      [[ "${2:-}" = /* ]] || fail '--prefix requires an absolute directory.' 2
      prefix="$2"
      shift 2
      ;;
    *) fail "Unknown option: $1 (see --help)." 2 ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || fail 'LarpCC installation requires macOS.'
[[ "${HOME:-}" = /* ]] || fail 'HOME must be an absolute directory.'
prefix="${prefix:-$HOME/.local/bin}"
binary="${prefix%/}/larp"
applications="$HOME/Applications"
app="$applications/LarpCC.app"
repo="$(cd -P -- "$(dirname -- "$0")/.." && pwd)"

if "$dry_run"; then
  printf 'Build in %q:\n' "$repo"
  printf '  cargo build --release --locked --bin larp --target-dir %q\n' "$repo/target"
  printf '  ./apps/macos/scripts/bundle.sh\n'
  printf 'Install larp at %q\nInstall unsigned app at %q\n' "$binary" "$app"
  exit 0
fi

if [[ -L "$binary" || ( -e "$binary" && ! -f "$binary" ) ]]; then
  fail "Expected a regular file or an unused path at $binary"
fi
if [[ -e "$app" || -L "$app" ]]; then
  if [[ -L "$app" || ! -d "$app" ]] ||
    [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null)" != dev.larpcc.settings ]]; then
    fail "Move the existing item at $app before installing LarpCC."
  fi
fi

cd "$repo"
cargo build --release --locked --bin larp --target-dir "$repo/target"
./apps/macos/scripts/bundle.sh

mkdir -p -m 0755 "$prefix" "$applications"
physical_prefix="$(cd -P -- "$prefix" && pwd)"
physical_app="$(cd -P -- "$applications" && pwd)/LarpCC.app"
case "$physical_prefix/" in
  "$physical_app/"*) fail '--prefix must be outside the installed app bundle.' 2 ;;
esac

bin_stage=''
app_stage=''
cleanup() {
  if [[ -n "$app_stage" ]]; then
    if [[ -d "$app_stage/previous/LarpCC.app" ]]; then
      if [[ -e "$app" || -L "$app" ]] || ! mv "$app_stage/previous/LarpCC.app" "$applications/"; then
        printf 'Previous app saved at %q\n' "$app_stage/previous/LarpCC.app" >&2
        app_stage=''
      fi
    fi
    if [[ -n "$app_stage" ]]; then
      rm -rf "$app_stage"
    fi
  fi
  if [[ -n "$bin_stage" ]]; then
    rm -rf "$bin_stage"
  fi
}
trap cleanup EXIT
bin_stage="$(mktemp -d "${prefix%/}/.larp-install.XXXXXXXX")"
app_stage="$(mktemp -d "$applications/.larp-install.XXXXXXXX")"
install -m 0755 target/release/larp "$bin_stage/larp"
ditto apps/macos/dist/LarpCC.app "$app_stage/LarpCC.app"

mv -fh "$bin_stage/larp" "$prefix/"
if [[ -d "$app" ]]; then
  mkdir "$app_stage/previous"
  mv "$app" "$app_stage/previous/"
fi
mv "$app_stage/LarpCC.app" "$applications/"
rm -rf "$app_stage/previous"

printf 'Installed %q and %q\n' "$binary" "$app"
printf 'If larp is missing from PATH, add this to your shell configuration:\n'
printf '  export PATH=%q:"$PATH"\n' "$prefix"
printf 'If macOS blocks your self-built app due to quarantine, run:\n'
printf '  xattr -dr com.apple.quarantine %q\n' "$app"
