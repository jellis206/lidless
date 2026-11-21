#!/bin/zsh
# Build Lidless.app for this Mac, signed ad hoc (local use only).
#   scripts/bundle.sh        install into /Applications and (re)start it
#   scripts/bundle.sh DIR    put the app in DIR instead and leave it alone, for testing
set -euo pipefail
cd "${0:A:h}/.."
DEST="${1:-/Applications}"
VER=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
APP="target/Lidless.app"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cargo build --release --locked
cp target/release/lidless "$APP/Contents/MacOS/Lidless"

ICONSET=$(mktemp -d)/Lidless.iconset; mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon-1024.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s*2)) $((s*2)) assets/icon-1024.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Lidless.icns"

cat > "$APP/Contents/Info.plist" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Lidless</string>
<key>CFBundleDisplayName</key><string>Lidless</string>
<key>CFBundleIdentifier</key><string>local.lidless</string>
<key>CFBundleExecutable</key><string>Lidless</string>
<key>CFBundleIconFile</key><string>Lidless</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VER</string>
<key>CFBundleVersion</key><string>$VER</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>LSUIElement</key><true/>
<key>NSHumanReadableCopyright</key><string>© 2026 Alex McConnell, John Ellis. MIT licence.</string>
</dict></plist>
PL

codesign --force --options runtime --sign - "$APP"
codesign --verify --strict "$APP"

mkdir -p "$DEST"
# Nothing changed: leave the running app alone rather than restart it for no reason.
if diff -rq "$APP" "$DEST/Lidless.app" >/dev/null 2>&1; then
  echo "Lidless $VER already up to date in $DEST"
  exit 0
fi
if [[ $# -eq 0 ]]; then
  pkill -x Lidless || true
fi
rm -rf "$DEST/Lidless.app" && cp -R "$APP" "$DEST/"
echo "Lidless $VER -> $DEST/Lidless.app"
if [[ $# -eq 0 ]]; then
  open "$DEST/Lidless.app"
fi
