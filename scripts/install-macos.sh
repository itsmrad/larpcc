#!/bin/bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: install-macos.sh [--prefix ABSOLUTE_DIRECTORY] [--replace] [--dry-run] [--help]

Build and install larp and the unsigned LarpCC settings app on macOS.
The binary defaults to ~/.local/bin/larp. The app goes to ~/Applications/LarpCC.app.
Use --replace to replace an unrecognized larp and keep a backup.
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
load_protected_roots() {
  local root physical
  physical_home="$(resolve_directory "$home")" || return 1
  protected_roots=("$home/Library/Application Support/larpcc" "$home/.agents")
  if [[ -n "${LARPCC_HOME:-}" ]]; then
    root="$(normalize_path "$LARPCC_HOME")" || return 1
    protected_roots+=("$root")
  fi
  for root in "${protected_roots[@]}"; do
    physical="$(resolve_directory "$root")" || return 1
    protected_roots+=("$physical")
  done
  for root in "$home"/.claude* "$home"/.codex*; do
    if [[ -L "$root" && -d "$root" ]]; then
      physical="$(resolve_directory "$root")" || return 1
      protected_roots+=("$physical")
    fi
  done
}
load_protected_roots

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

owned_binary_digest() {
  local recorded actual
  [[ ! -L "$1" && -f "$1" ]] || return 1
  recorded="$(xattr -p "$marker" "$1" 2>/dev/null)" || return 1
  actual="$(digest "$1")" || return 1
  [[ -n "$actual" && "$recorded" == "$actual" ]] || return 1
  printf '%s\n' "$actual"
}

owned_app() {
  local bundle="$1" plist="$1/Contents/Info.plist"
  [[ ! -L "$bundle" && -d "$bundle" && ! -L "$bundle/Contents" && ! -L "$plist" && -f "$plist" ]] &&
    [[ "$(plutil -extract CFBundleIdentifier raw -o - "$plist" 2>/dev/null)" == dev.larpcc.settings ]]
}

target_state() {
  local target="$1" kind="$2" hash='' stamp
  [[ -e "$target" || -L "$target" ]] || return 0
  if [[ "$kind" == binary ]]; then
    [[ ! -L "$target" && -f "$target" ]] ||
      fail "Expected a regular file or an unused path at $target"
    if ! "$replace" && ! hash="$(owned_binary_digest "$target")"; then
      fail "Unrecognized larp at $target. Move it aside or explicitly use --replace."
    fi
  else
    owned_app "$target" || fail "Move the existing item at $target before installing LarpCC."
    hash="$(digest "$target/Contents/Info.plist")" || return 1
  fi
  stamp="$(stat -f '%d:%i:%z:%m:%c' "$target")" || return 1
  printf '%s:%s\n' "$stamp" "$hash"
}

bin_before="$(target_state "$binary" binary)"
app_before="$(target_state "$app" app)"
if "$dry_run"; then
  printf 'Build in %q:\n' "$repo"
  printf '  cargo build --release --locked --bin larp --target-dir %q\n' "$repo/target"
  printf '  ./apps/macos/scripts/bundle.sh\n'
  printf 'Install larp at %q\nInstall unsigned app at %q\n' "$binary" "$app"
  exit 0
fi

mkdir -p -m 0755 "$prefix" "$applications"

directory_id() (
  local directory="$1" allow_system="$2" target="$3" kind="$4" resolved
  load_protected_roots || exit 1
  resolved="$(resolve_destination "$directory" "$allow_system")" || exit 1
  [[ "$resolved" == "$directory" ]] || fail "Installation directory moved: $directory"
  reject_protected "$target" "$kind" || exit 1
  if [[ "$kind" == app ]]; then reject_protected "$lock" directory || exit 1; fi
  stat -f '%d:%i' "$resolved"
)

prefix_id="$(directory_id "$prefix" "$explicit_prefix" "$binary" file)"
applications_id="$(directory_id "$applications" false "$app" app)"
check_prefix() {
  [[ "$(directory_id "$prefix" "$explicit_prefix" "$binary" file)" == "$prefix_id" ]]
}

check_applications() {
  [[ "$(directory_id "$applications" false "$app" app)" == "$applications_id" ]]
}

require_directory() {
  "$1" || fail "Installation directory changed: $2"
}

mkdir "$lock" || fail "Another installation may be running. If none is active, remove the empty lock at $lock."
bin_stage=''
app_stage=''
publishing_bin=''
publishing_app=''
committed=false

clean_target() {
  local stage="$1" target="$2" parent="$3" publishing="$4" kind="$5" name="${2##*/}" backup
  [[ -n "$stage" ]] || return 0
  backup="$stage/previous/$name"
  if ! "$committed"; then
    if [[ -n "$publishing" && ! -e "$stage/$name" ]]; then
      [[ "$(stat -f '%d:%i' "$target")" == "$publishing" ]] || return 1
      mv -fh "$target" "$stage/" || return 1
    fi
    if [[ -e "$backup" || -L "$backup" ]]; then
      [[ ! -e "$target" && ! -L "$target" ]] &&
        mv -nh "$backup" "$parent/" && [[ ! -e "$backup" && ! -L "$backup" ]] || return 1
    fi
  elif [[ -e "$backup" || -L "$backup" ]]; then
    if [[ "$kind" == binary ]]; then
      if ! owned_binary_digest "$backup" >/dev/null; then
        printf 'Kept previous CLI at %q\n' "$backup" >&2
        return 0
      fi
    elif ! owned_app "$backup"; then
      printf 'Kept previous app at %q\n' "$backup" >&2
      return 0
    fi
  fi
  rm -rf "$stage"
}

cleanup() {
  if [[ -n "$bin_stage" ]]; then
    if ! check_prefix || ! clean_target "$bin_stage" "$binary" "$prefix" "$publishing_bin" binary; then
      printf 'CLI cleanup failed. Look for %q in the original directory.\n' "${bin_stage##*/}" >&2
    fi
  fi
  if check_applications; then
    clean_target "$app_stage" "$app" "$applications" "$publishing_app" app ||
      printf 'App cleanup failed. Recover files from %q\n' "$app_stage" >&2
    rmdir "$lock"
  else
    printf 'App directory changed. The install lock and any staged files remain in the original directory.\n' >&2
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cd "$repo"
cargo build --release --locked --bin larp --target-dir "$repo/target"
./apps/macos/scripts/bundle.sh

# shortcut: shell checks catch changes during the build, but check and move are separate operations.
require_directory check_prefix "$prefix"
require_directory check_applications "$applications"

bin_stage="$(mktemp -d "${prefix%/}/.larp-install.XXXXXXXX")"
app_stage="$(mktemp -d "$applications/.larp-install.XXXXXXXX")"
install -m 0755 target/release/larp "$bin_stage/larp"
hash="$(digest "$bin_stage/larp")"
xattr -w "$marker" "$hash" "$bin_stage/larp"
ditto apps/macos/dist/LarpCC.app "$app_stage/LarpCC.app"

backup_target() {
  local target="$1" kind="$2" stage="$3" before="$4" current
  mkdir "$stage/previous"
  current="$(target_state "$target" "$kind")"
  if ! "$replace" && [[ "$current" != "$before" ]]; then
    fail "Installation target changed during the build: $target"
  fi
  if [[ -n "$current" ]]; then
    mv -fh "$target" "$stage/previous/"
  fi
}

require_directory check_prefix "$prefix"
backup_target "$binary" binary "$bin_stage" "$bin_before"
require_directory check_applications "$applications"
backup_target "$app" app "$app_stage" "$app_before"

publishing_bin="$(stat -f '%d:%i' "$bin_stage/larp")"
require_directory check_prefix "$prefix"
mv -nh "$bin_stage/larp" "$prefix/"
[[ ! -e "$bin_stage/larp" ]] || fail "Installation target appeared: $binary"
publishing_app="$(stat -f '%d:%i' "$app_stage/LarpCC.app")"
require_directory check_applications "$applications"
mv -nh "$app_stage/LarpCC.app" "$applications/"
[[ ! -e "$app_stage/LarpCC.app" ]] || fail "Installation target appeared: $app"
committed=true

printf 'Installed %q and %q\n' "$binary" "$app"
printf 'If larp is missing from PATH, add this to your shell configuration:\n'
printf '  export PATH=%q:"$PATH"\n' "$prefix"
printf 'If macOS blocks your self-built app due to quarantine, run:\n'
printf '  xattr -dr com.apple.quarantine %q\n' "$app"
