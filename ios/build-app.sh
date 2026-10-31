#!/bin/bash
# Builds Horizon.app for an iOS target into target/ios/<target>/Payload/Horizon.app.
#   bash ios/build-app.sh [aarch64-apple-ios | aarch64-apple-ios-sim] [cargo profile]
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET=${1:-aarch64-apple-ios}
PROFILE=${2:-release}
VERSION=${HORIZON_IOS_VERSION:-0.1.0}
BUILD=${HORIZON_IOS_BUILD:-1}
export IPHONEOS_DEPLOYMENT_TARGET=15.0

rustup target add "$TARGET"
# Audio, no controllers: gilrs and the DualSense's HID have no iOS backend.
cargo build --locked -p horizon-client --bin horizon-client --profile "$PROFILE" \
  --target "$TARGET" --no-default-features --features audio

out="target/ios/$TARGET"
app="$out/Payload/Horizon.app"
rm -rf "$out"
mkdir -p "$app"
cp "target/$TARGET/$PROFILE/horizon-client" "$app/horizon-client"
platform=iPhoneOS
[ "$TARGET" = aarch64-apple-ios-sim ] && platform=iPhoneSimulator
sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" -e "s/<string>iPhoneOS<\/string>/<string>$platform<\/string>/" \
  ios/Info.plist > "$app/Info.plist"
plutil -lint "$app/Info.plist"
icon=docs/media/brand/icon-1024.png
for spec in AppIcon60x60@2x:120 AppIcon60x60@3x:180 AppIcon76x76@2x:152; do
  sips -z "${spec#*:}" "${spec#*:}" "$icon" --out "$app/${spec%:*}.png" >/dev/null
done
printf 'APPL????' > "$app/PkgInfo"
echo "$app"
