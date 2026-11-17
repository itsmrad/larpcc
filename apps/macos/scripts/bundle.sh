#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'LarpCC bundling requires macOS.' >&2
  exit 1
fi
swift build -c release --product LarpCC
bin_dir="$(swift build -c release --show-bin-path)"
# Only create/replace the executable and metadata inside this package's output.
app='dist/LarpCC.app'
mkdir -p "$app/Contents/MacOS"
cp "$bin_dir/LarpCC" "$app/Contents/MacOS/LarpCC"
# SwiftPM resolves resources next to the executable in an unsigned app bundle.
find "$bin_dir" -maxdepth 1 -name '*.bundle' -exec cp -R {} "$app/Contents/MacOS/" \;
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
