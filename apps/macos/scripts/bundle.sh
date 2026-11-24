#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'LarpCC bundling requires macOS.' >&2
  exit 1
fi
# SwiftPM records the deployment target as the SDK version, which makes macOS
# draw the app in its compatibility style. Record the real SDK instead.
if ! sdk_version="$(xcrun --show-sdk-version)" || [[ -z "$sdk_version" ]]; then
  echo 'Could not determine the macOS SDK version.' >&2
  exit 1
fi
swift build -c release --product LarpCC \
  -Xlinker -platform_version -Xlinker macos -Xlinker 14.0 -Xlinker "$sdk_version"
bin_dir="$(swift build -c release --show-bin-path)"
# Only create/replace the executable and metadata inside this package's output.
app='dist/LarpCC.app'
mkdir -p "$app/Contents/MacOS"
# Replace rather than overwrite, so a running copy keeps its own signed file.
rm -f "$app/Contents/MacOS/LarpCC"
cp "$bin_dir/LarpCC" "$app/Contents/MacOS/LarpCC"
# SwiftPM Bundle.module resolves its bundle from Bundle.main.bundleURL.
find "$bin_dir" -maxdepth 1 -name '*.bundle' -exec cp -R {} "$app/" \;
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.larpcc.settings</string>
<key>CFBundleName</key><string>LarpCC</string>
<key>CFBundleDisplayName</key><string>LarpCC</string>
<key>CFBundleExecutable</key><string>LarpCC</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
echo "Unsigned app: $(pwd)/$app"
