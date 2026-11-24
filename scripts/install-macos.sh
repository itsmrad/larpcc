#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install-macos.sh [--prefix ABSOLUTE_DIRECTORY] [--replace] [--dry-run] [--help]

Build and install larp and the unsigned LarpCC settings app on macOS.
The binary defaults to ~/.local/bin/larp. The app goes to ~/Applications/LarpCC.app.
Use --replace to replace an existing larp that this installer cannot recognize.
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
replace=false
while (($#)); do
  case "$1" in
    --help) usage; exit 0 ;;
    --dry-run) dry_run=true; shift ;;
    --replace) replace=true; shift ;;
    --prefix)
      [[ "${2:-}" = /* ]] || fail '--prefix requires an absolute directory.' 2
      prefix="$2"
      shift 2
      ;;
    *) fail "Unknown option: $1 (see --help)." 2 ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || fail 'LarpCC installation requires macOS.'
repo="$(cd -P -- "$(dirname -- "$0")/.." && pwd)"

normalize_path() {
  [[ "$1" == /* && "$1" != *[[:cntrl:]]* ]] || fail "Expected an absolute directory path: $1" 2
  local rest="${1#/}" part path=''
  while [[ -n "$rest" ]]; do
    part="${rest%%/*}"
    if [[ "$rest" == */* ]]; then rest="${rest#*/}"; else rest=''; fi
    case "$part" in
      ''|.) continue ;;
      ..) fail "Parent components are not supported: $1" 2 ;;
      *) path="$path/$part" ;;
    esac
  done
  printf '%s\n' "${path:-/}"
}

resolve_directory() {
  local ancestor="$1" tail='' physical
  while [[ ! -L "$ancestor" && ! -e "$ancestor" ]]; do
    tail="/${ancestor##*/}$tail"
    ancestor="${ancestor%/*}"
    ancestor="${ancestor:-/}"
  done
  physical="$(cd -P -- "$ancestor" && pwd -P)" || fail "Cannot resolve directory: $ancestor"
  [[ "$physical" != *[[:cntrl:]]* ]] || fail "Unsupported directory path: $ancestor"
  physical="${physical%/}$tail"
  printf '%s\n' "${physical:-/}"
}

reject_protected() (
  shopt -s nocasematch
  local path="$1" protected
  # For a bundle replacement, also protect state configured inside the bundle.
  for protected in "${protected_roots[@]}"; do
    if [[ "$path" == "$protected" || "$path" == "${protected%/}/"* ||
          ( "$2" == app && "$protected" == "${path%/}/"* ) ]]; then
      fail "Installation would affect protected data: $path"
    fi
  done
  case "$path" in
    "$home"/.claude*|"$home"/.codex*|"$physical_home"/.claude*|"$physical_home"/.codex*)
      fail "Installation destination is in a provider home: $path" ;;
  esac
)

resolve_destination() (
  shopt -s nocasematch
  local path="$1" allow_system="$2" current='' rest="${1#/}" part
  reject_protected "$path" directory || exit 1
  case "$path/" in
    *'.app/'*) fail "Installation directory is in an app bundle: $path" ;;
  esac
  while [[ -n "$rest" ]]; do
    part="${rest%%/*}"
    if [[ "$rest" == */* ]]; then rest="${rest#*/}"; else rest=''; fi
    current="$current/$part"
    [[ ! -L "$current" ]] || fail "Installation destination contains a symlink: $current"
  done
  path="$(resolve_directory "$path")" || exit 1
  reject_protected "$path" directory || exit 1
  if ! "$allow_system"; then
    case "${path%/}/" in
      /|/Applications/*|/Library/*|/System/*|/bin/*|/sbin/*|/usr/*|/etc/*|/private/*|/var/*|/dev/*)
        fail "Installation destination requires an explicit --prefix: $path" ;;
    esac
  fi
  printf '%s\n' "$path"
)

home="$(normalize_path "${HOME:-}")"
physical_home="$(resolve_directory "$home")"
default_root="$home/Library/Application Support/larpcc"
physical_default_root="$(resolve_directory "$default_root")"
physical_agents="$(resolve_directory "$home/.agents")"
protected_roots=("$default_root" "$physical_default_root" "$home/.agents" "$physical_agents")
if [[ -n "${LARPCC_HOME:-}" ]]; then
  data_root="$(normalize_path "$LARPCC_HOME")"
  physical_data_root="$(resolve_directory "$data_root")"
  protected_roots+=("$data_root" "$physical_data_root")
fi
for protected in "$home"/.claude* "$home"/.codex*; do
  if [[ -L "$protected" && -d "$protected" ]]; then
    physical_provider_root="$(resolve_directory "$protected")"
    protected_roots+=("$physical_provider_root")
  fi
done

explicit_prefix=false
if [[ -n "$prefix" ]]; then explicit_prefix=true; fi
prefix="$(normalize_path "${prefix:-$home/.local/bin}")"
prefix="$(resolve_destination "$prefix" "$explicit_prefix")"
applications="$(resolve_destination "$home/Applications" false)"
binary="${prefix%/}/larp"
app="$applications/LarpCC.app"
lock="$applications/.larp-install.lock"
reject_protected "$binary" file
reject_protected "$app" app
reject_protected "$lock" directory

marker=dev.larpcc.installer.sha256
digest() {
  local hash
  hash="$(shasum -a 256 < "$1")" || return
  printf '%s\n' "${hash%% *}"
}

check_existing() {
  if [[ -L "$binary" || ( -e "$binary" && ! -f "$binary" ) ]]; then
    fail "Expected a regular file or an unused path at $binary"
  fi
  if [[ -f "$binary" ]] && ! "$replace"; then
    local recorded actual
    if ! recorded="$(xattr -p "$marker" "$binary" 2>/dev/null)" ||
      ! actual="$(digest "$binary")" || [[ -z "$actual" || "$actual" != "$recorded" ]]; then
      fail "Unrecognized larp at $binary. Move it aside or explicitly use --replace."
    fi
  fi
  if [[ -e "$app" || -L "$app" ]]; then
    if [[ -L "$app" || ! -d "$app" || -L "$app/Contents" || -L "$app/Contents/Info.plist" || ! -f "$app/Contents/Info.plist" ]] ||
      [[ "$(plutil -extract CFBundleIdentifier raw -o - "$app/Contents/Info.plist" 2>/dev/null)" != dev.larpcc.settings ]]; then
      fail "Move the existing item at $app before installing LarpCC."
    fi
  fi
}

check_existing
if "$dry_run"; then
  printf 'Build in %q:\n' "$repo"
  printf '  cargo build --release --locked --bin larp --target-dir %q\n' "$repo/target"
  printf '  ./apps/macos/scripts/bundle.sh\n'
  printf 'Install larp at %q\nInstall unsigned app at %q\n' "$binary" "$app"
  exit 0
fi

mkdir -p -m 0755 "$prefix" "$applications"
mkdir "$lock" || fail "Another installation may be running. If none is active, remove the empty lock at $lock."
bin_stage=''
app_stage=''
publishing_bin=false
publishing_app=false
committed=false

clean_target() {
  local stage="$1" target="$2" parent="$3" publishing="$4" name="${2##*/}"
  [[ -n "$stage" ]] || return 0
  if ! "$committed"; then
    if "$publishing" && [[ ! -e "$stage/$name" ]]; then
      mv -fh "$target" "$stage/" || return 1
    fi
    if [[ -e "$stage/previous/$name" ]]; then
      [[ ! -e "$target" && ! -L "$target" ]] &&
        mv -fh "$stage/previous/$name" "$parent/" || return 1
    fi
  fi
  rm -rf "$stage"
}

cleanup() {
  clean_target "$app_stage" "$app" "$applications" "$publishing_app" ||
    printf 'App cleanup failed. Recover files from %q\n' "$app_stage" >&2
  clean_target "$bin_stage" "$binary" "$prefix" "$publishing_bin" ||
    printf 'CLI cleanup failed. Recover files from %q\n' "$bin_stage" >&2
  rmdir "$lock"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

check_existing
cd "$repo"
cargo build --release --locked --bin larp --target-dir "$repo/target"
./apps/macos/scripts/bundle.sh

bin_stage="$(mktemp -d "${prefix%/}/.larp-install.XXXXXXXX")"
app_stage="$(mktemp -d "$applications/.larp-install.XXXXXXXX")"
install -m 0755 target/release/larp "$bin_stage/larp"
hash="$(digest "$bin_stage/larp")"
xattr -w "$marker" "$hash" "$bin_stage/larp"
ditto apps/macos/dist/LarpCC.app "$app_stage/LarpCC.app"

if [[ -f "$binary" ]]; then
  mkdir "$bin_stage/previous"
  mv -fh "$binary" "$bin_stage/previous/"
fi
if [[ -d "$app" ]]; then
  mkdir "$app_stage/previous"
  mv -fh "$app" "$app_stage/previous/"
fi
publishing_bin=true
mv -fh "$bin_stage/larp" "$prefix/"
publishing_app=true
mv -fh "$app_stage/LarpCC.app" "$applications/"
committed=true

printf 'Installed %q and %q\n' "$binary" "$app"
printf 'If larp is missing from PATH, add this to your shell configuration:\n'
printf '  export PATH=%q:"$PATH"\n' "$prefix"
printf 'If macOS blocks your self-built app due to quarantine, run:\n'
printf '  xattr -dr com.apple.quarantine %q\n' "$app"
