#!/bin/bash
# Builds Horizon.ipa, unsigned, for iPhone and iPad (arm64, iOS 15+). Runs on macOS with Xcode and rustup;
# ios.yml runs it on GitHub. Sign it to install it: Sideloadly, AltStore or TrollStore. docs/ios.md.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=${HORIZON_IOS_VERSION:-0.1.0}
BUILD=${HORIZON_IOS_BUILD:-1}
PROFILE=${HORIZON_IOS_PROFILE:-release}
TARGET=aarch64-apple-ios
export IPHONEOS_DEPLOYMENT_TARGET=15.0

rustup target add "$TARGET"
# Audio, no controllers: gilrs and the DualSense's HID have no iOS backend.
cargo build --locked -p horizon-client --bin horizon-client --profile "$PROFILE" \
  --target "$TARGET" --no-default-features --features audio

out=target/ios
app="$out/Payload/Horizon.app"
rm -rf "$out"
mkdir -p "$app"
cp "target/$TARGET/$PROFILE/horizon-client" "$app/horizon-client"
sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" ios/Info.plist > "$app/Info.plist"
plutil -lint "$app/Info.plist"
icon=docs/media/brand/icon-1024.png
for spec in AppIcon60x60@2x:120 AppIcon60x60@3x:180 AppIcon76x76@2x:152; do
  sips -z "${spec#*:}" "${spec#*:}" "$icon" --out "$app/${spec%:*}.png" >/dev/null
done
printf 'APPL????' > "$app/PkgInfo"
# Unsigned: no _CodeSignature. The binary keeps its linker's ad-hoc signature, which signers replace.
(cd "$out" && zip -qry Horizon.ipa Payload)
ls -lh "$out/Horizon.ipa"
