# Horizon on iPhone and iPad

The iOS app is the game itself (`horizon-client`, built for `aarch64-apple-ios`) in an app bundle, with touch
controls. It renders with Metal through wgpu, as on macOS. Rust compiles ahead of time, so the app needs no JIT,
unlike Java launchers on iOS.

## Get the IPA

The IPA is built on macOS:

- **GitHub:** run the `ios` workflow from the Actions tab. Each run uploads `Horizon-ipa`. Run it by hand with
  `release` on and it also publishes the `ios-latest` prerelease, which has a direct download link.
- **A Mac:** `bash ios/build-ipa.sh` writes `target/ios/Horizon.ipa`.

The IPA is **unsigned**. Install it with a tool that signs it: Sideloadly or AltStore with your Apple ID, or
TrollStore where it works. `ios/entitlements.plist` lists the increased memory limit to request when signing.

## Add your game files

Horizon needs your own copy of Minecraft: Java Edition. The prepared files come from the preparer, which runs
Mojang's own Java code, and iOS can't run that. So you prepare them on a computer:

1. On a computer, install the Horizon launcher, sign in and press **Play** once. This prepares the files.
2. Zip them:
   - Windows: `powershell -ExecutionPolicy Bypass -File ios\export-resources.ps1`
   - macOS and Linux: `bash ios/export-resources.sh`

   Both write `horizon-resources.zip` to the Desktop.
3. Move the zip to the iPhone (AirDrop, iCloud Drive, a cable). Put it in **Files › On My iPhone › Horizon** and
   tap it to unzip it.
4. Open Horizon. It looks up to four folders deep in its Documents for `horizon-resources.json`.

The Documents folder is the game folder. `saves/`, `options.txt`, `resourcepacks/`, `screenshots/` and `logs/` are
all there, visible in Files.

## Accounts

- **Offline:** single player and LAN work under the offline name you pick in the game.
- **Online-mode servers:** copy a session from the launcher on the computer as `session.json` into the Horizon
  folder. The game reads it once and deletes it. Minecraft tokens last about a day.

The app has no Microsoft sign-in of its own. That needs the launcher's Azure app (`HORIZON_MSA_CLIENT_ID`), which
this source doesn't contain.

## Touch controls

| Where | What |
|---|---|
| Lower left | Move stick (`game/touch.rs`). Push it to the edge to sprint. |
| Drag elsewhere | Look around |
| Tap | Use or place (right click) |
| Press and hold | Mine or attack (left click) |
| Hotbar | Tap a slot to select it |
| Buttons | Jump, Sneak (toggle), Hit, Use, Inv, Drop, View (F5), Chat, Back (pause) |
| Menus | One finger is the mouse; two fingers scroll |
| Text fields | The on-screen keyboard opens by itself. **Keys** opens or closes it. |

A keyboard, mouse or trackpad on an iPad works as on a computer.

## What differs from the desktop

- There is no launcher UI on iOS. The game starts straight to its title screen, with Documents as the game folder.
  Instances, mods built by the launcher, modpack import and updates stay on the computer.
- The clipboard stays inside the game.
- Controllers aren't supported yet: gilrs has no iOS backend.
- The narrator is off. The macOS one uses AppKit.

## Building on Windows (type checks only)

You can't link an iOS app without Xcode. `cargo check --target aarch64-apple-ios` still works on Windows if you
give cc-rs a stand-in C compiler and SDK root (`CC_aarch64_apple_ios`, `AR_aarch64_apple_ios`, `SDKROOT`). This
checks the Rust and links nothing.
