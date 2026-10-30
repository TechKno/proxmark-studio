#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release --target aarch64-apple-darwin --bins
bundle="dist/Proxmark Studio.app"
target_dir="${CARGO_TARGET_DIR:-target}"
mkdir -p "$bundle/Contents/MacOS"
cp "$target_dir/aarch64-apple-darwin/release/proxmark-studio" "$bundle/Contents/MacOS/proxmark-studio"
cp "$target_dir/aarch64-apple-darwin/release/mock-pm3" "$bundle/Contents/MacOS/mock-pm3"
cp assets/Info.plist "$bundle/Contents/Info.plist"
# Local ad-hoc signature only. Developer ID signing/notarisation needs your identity.
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' assets/Info.plist)
package="dist/ProxmarkStudio-$version-macOS-arm64"
mkdir -p "$package/examples"
cp -R "$bundle" "$package/"
cp README.md LICENSE VALIDATION.md ORGANISATIONS.md SECURITY.md "$package/"
cp examples/synthetic-users.csv examples/synthetic-organisation-profile.json "$package/examples/"
# ditto preserves the executable modes and app bundle in the downloadable ZIP.
ditto -c -k --keepParent "$package" "$package.zip"
printf '%s\n' "Created $package.zip (Apple Silicon; ad-hoc signed; not notarised)"
