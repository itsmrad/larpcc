#!/bin/bash
# Renders every Settings page, the menu bar panel and the status item in each theme and
# appearance with sample data into <output dir>, plus index.md. Debug build; macOS only.
# Usage: scripts/snapshots.sh <output dir>   (SNAPSHOT_THEMES="tahoe sonoma" to limit)
set -uo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'LarpCC snapshots require macOS.' >&2
  exit 1
fi
out="${1:?usage: scripts/snapshots.sh <output dir>}"
mkdir -p "$out" && out="$(cd "$out" && pwd)" || exit 1

# Link like scripts/bundle.sh, or macOS draws the compatibility style (small window corners).
swift build --product LarpCC \
  -Xlinker -platform_version -Xlinker macos -Xlinker 14.0 -Xlinker "$(xcrun --show-sdk-version)" || exit 1
bin="$(swift build --show-bin-path)/LarpCC"

os="$(sw_vers -productVersion)"
os_major="${os%%.*}"
themes=()
skipped=''
for theme in ${SNAPSHOT_THEMES:-tahoe sequoia sonoma}; do
  # The app ignores --theme=tahoe before macOS 26; skip it rather than mislabel the native theme.
  if [[ "$theme" == tahoe && "$os_major" -lt 26 ]]; then skipped+=" tahoe"; continue; fi
  themes+=("$theme")
done

pages=(general mcp skills aliases desktop about)
# Every profile in the sample data, so new sample profiles are picked up automatically.
if ! profile_ids="$(perl -e 'alarm shift; exec @ARGV' 30 "$bin" --sample-profile-ids)" || [[ -z "$profile_ids" ]]; then
  echo 'Could not list sample profiles.' >&2
  exit 1
fi
while IFS= read -r id; do
  [[ -n "$id" ]] && pages+=("profile-$id")
done <<< "$profile_ids"

log="$out/render.log"
: > "$log"
failed=()
# Runs the app with a time limit and records its stderr.
render() {
  local name="$1"; shift
  if perl -e 'alarm shift; exec @ARGV' 60 "$bin" -ApplePersistenceIgnoreState YES "$@" 2> >(sed "s|^|$name: |" >> "$log"); then
    echo "ok   $name"
  else
    echo "FAIL $name"; failed+=("$name")
  fi
}

for theme in "${themes[@]}"; do
  for appearance in light dark; do
    common=(--theme="$theme" --appearance="$appearance")
    for page in "${pages[@]}"; do
      render "settings-$page-$theme-$appearance.png" "${common[@]}" --page="$page" \
        --settings-snapshot="$out/settings-$page-$theme-$appearance.png"
    done
    panel="--menubar-panel-snapshot=$out/panel"
    status=()
    [[ "$appearance" == light ]] && status=(--menubar-status-snapshot="$out/status-item-$theme-lightdark.png")
    render "panel-collapsed-$theme-$appearance.png" "${common[@]}" "$panel-collapsed-$theme-$appearance.png" ${status[@]+"${status[@]}"}
    render "panel-expanded-$theme-$appearance.png" "${common[@]}" "$panel-expanded-$theme-$appearance.png" --menubar-panel-expanded=1
    render "panel-selected-$theme-$appearance.png" "${common[@]}" "$panel-selected-$theme-$appearance.png" --menubar-panel-selected=1
    render "panel-short-$theme-$appearance.png" "${common[@]}" "$panel-short-$theme-$appearance.png" \
      --menubar-panel-expanded=1 --menubar-screen-height=500
    render "panel-accessibility-$theme-$appearance.png" "${common[@]}" "$panel-accessibility-$theme-$appearance.png" \
      --menubar-panel-accessibility
  done
done

{
  echo '# LarpCC snapshots'
  echo
  echo "- macOS $os ($(uname -m)), Swift $(swift --version 2>/dev/null | head -1 | sed 's/.*version \([0-9.]*\).*/\1/')"
  echo "- Themes drawn on this macOS: ${themes[*]}${skipped:+ (skipped:$skipped, needs macOS 26)}"
  echo "- Settings capture: $(grep -o 'method=[a-z]*' "$log" | sort | uniq -c | awk '{printf "%s%s x%s", sep, $2, $1; sep=", "}')"
  echo "- Sample data only (FixtureLarpClient). Status item images hold a light bar above a dark bar."
  [[ ${#failed[@]} -gt 0 ]] && echo "- FAILED: ${failed[*]} (see render.log)"
  echo
  echo '## Files'
  echo
  (cd "$out" && ls -1 *.png 2>/dev/null) | sed 's/^/- /'
} > "$out/index.md"

echo "$(ls -1 "$out"/*.png 2>/dev/null | wc -l | tr -d ' ') PNGs in $out"
if [[ ${#failed[@]} -gt 0 ]]; then
  echo "Failed: ${failed[*]}" >&2
  exit 1
fi
