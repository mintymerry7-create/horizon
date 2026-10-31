#!/bin/bash
# Builds Horizon.ipa, unsigned, for iPhone and iPad (arm64, iOS 15+). Runs on macOS with Xcode and rustup;
# ios.yml runs it on GitHub. Sign it to install it: Sideloadly, AltStore or TrollStore. docs/ios.md.
set -euo pipefail
cd "$(dirname "$0")/.."
bash ios/build-app.sh aarch64-apple-ios "${HORIZON_IOS_PROFILE:-release}"
out=target/ios/aarch64-apple-ios
# Unsigned: no _CodeSignature. The binary keeps its linker's ad-hoc signature, which signers replace.
(cd "$out" && zip -qry Horizon.ipa Payload)
mv "$out/Horizon.ipa" target/ios/Horizon.ipa
ls -lh target/ios/Horizon.ipa
