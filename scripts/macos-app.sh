#!/bin/sh
# Builds B-Downloader.app for macOS, for both Apple Silicon and Intel Macs,
# and zips it as dist/B-Downloader-<version>-macos.zip.
#
#   sh scripts/macos-app.sh            # both kinds of Mac (universal)
#   sh scripts/macos-app.sh --native   # only this Mac's kind, faster for testing
#
# The app is signed ad hoc (not with an Apple Developer ID): macOS runs it,
# but the first time only after a right-click > Open, or when installed by
# Homebrew, which takes care of that.

set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
app=dist/B-Downloader.app
bins="BDownloader bdl-host"

if [ "${1:-}" = "--native" ]; then
    cargo build --release -p bdownloader -p bdownloader-host
    out=target/release
else
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --release -p bdownloader -p bdownloader-host --target "$target"
    done
    out=target/universal
    mkdir -p "$out"
    for bin in $bins; do
        lipo -create -output "$out/$bin" \
            "target/aarch64-apple-darwin/release/$bin" "target/x86_64-apple-darwin/release/$bin"
    done
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for bin in $bins; do
    cp "$out/$bin" "$app/Contents/MacOS/"
done
cp crates/app/assets/bdownloader.icns "$app/Contents/Resources/"
cp -R extension "$app/Contents/Resources/extension"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.ahmadanjum.bdownloader</string>
    <key>CFBundleName</key>
    <string>B-Downloader</string>
    <key>CFBundleDisplayName</key>
    <string>B-Downloader</string>
    <key>CFBundleExecutable</key>
    <string>BDownloader</string>
    <key>CFBundleIconFile</key>
    <string>bdownloader</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Ahmad Anjum</string>
</dict>
</plist>
EOF

# Ad hoc: the bridge first, then the app around it.
codesign --force --sign - "$app/Contents/MacOS/bdl-host"
codesign --force --sign - "$app"
codesign --verify --strict "$app"

zip="dist/B-Downloader-$version-macos.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
echo "$app"
echo "$zip ($(du -h "$zip" | cut -f1))"
