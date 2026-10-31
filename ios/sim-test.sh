#!/bin/bash
# Runs a Simulator build of Horizon.app in the iOS Simulator and keeps what it showed: a screenshot every
# 10 seconds, the game's logs, its output and any crash report, in OUT.
#   bash ios/sim-test.sh APP OUT [RESOURCES]
# Without RESOURCES: the missing-files screen. With them (a prepared horizon-client/26.3 folder): the title
# screen, then a new world for 1500 frames with its smoke report, as game-build.yml's smoke test.
set -uo pipefail
APP=$1
OUT=$2
RES=${3:-}
BUNDLE=dev.playhorizon.ios
mkdir -p "$OUT"

# An iPhone of the newest runtime installed.
UDID=$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
ios = sorted((r for r in devices if "iOS" in r), key=lambda r: [int(x) for x in r.rsplit("iOS-", 1)[1].split("-")])
for runtime in reversed(ios):
    phones = [d for d in devices[runtime] if d["name"].startswith("iPhone")]
    if phones:
        pro = [d for d in phones if "Pro" in d["name"]]
        d = (pro or phones)[0]
        print(d["udid"])
        print(d["name"], runtime, file=sys.stderr)
        break
')
[ -n "$UDID" ] || { echo "no iPhone simulator" >&2; exit 1; }
xcrun simctl boot "$UDID" 2>/dev/null
xcrun simctl bootstatus "$UDID" -b >/dev/null
codesign --force --sign - "$APP"
xcrun simctl install "$UDID" "$APP"
DOCS="$(xcrun simctl get_app_container "$UDID" "$BUNDLE" data)/Documents"
mkdir -p "$DOCS"
echo "Documents: $DOCS"

# limit SECONDS COMMAND...: macOS has no timeout(1). A hung simctl call must not hang the run.
limit() { perl -e 'alarm shift; exec @ARGV' "$@"; }
running() { limit 15 xcrun simctl spawn "$UDID" launchctl list 2>/dev/null | grep -q "$BUNDLE"; }

# run NAME SECONDS [game options...]: launch, screenshot every 10 s while it runs, then collect.
run() {
  local name=$1 secs=$2
  shift 2
  echo "== $name: up to ${secs}s, options: $*"
  limit 30 xcrun simctl terminate "$UDID" "$BUNDLE" 2>/dev/null
  rm -rf "$DOCS/logs" "$DOCS/crash-reports"
  SIMCTL_CHILD_HORIZON_IOS_ARGS="$*" limit 60 xcrun simctl launch --terminate-running-process \
    --stdout="$OUT/$name.stdout.log" --stderr="$OUT/$name.stderr.log" "$UDID" "$BUNDLE"
  local t=0
  while [ "$t" -lt "$secs" ]; do
    sleep 10
    t=$((t + 10))
    if ! running; then
      echo "   the app ended after about ${t}s"
      break
    fi
    limit 30 xcrun simctl io "$UDID" screenshot "$OUT/$name-$(printf %03d "$t")s.png" >/dev/null 2>&1
    # The log so far, in case the run never gets to the end.
    cp "$DOCS/logs/latest.log" "$OUT/$name.latest.log" 2>/dev/null
  done
  running && echo "   still running at ${secs}s"
  limit 30 xcrun simctl terminate "$UDID" "$BUNDLE" 2>/dev/null
  cp "$DOCS/logs/latest.log" "$OUT/$name.latest.log" 2>/dev/null
  cp -R "$DOCS/crash-reports" "$OUT/$name-crash-reports" 2>/dev/null
  tail -25 "$OUT/$name.latest.log" 2>/dev/null
  tail -15 "$OUT/$name.stderr.log" 2>/dev/null
}

run missing-files 30

if [ -n "$RES" ]; then
  cp -R "$RES" "$DOCS/resources"
  run title 60
  run world 300 --world smoke --seed 1 --render-distance 4 --world-frames 1500 --timeout-seconds 280 \
    --smoke-report "$DOCS/smoke.json"
  cp "$DOCS/smoke.json" "$OUT/smoke.json" 2>/dev/null && cat "$OUT/smoke.json"
fi

# Crashes the game's own handler missed (a native fault).
find ~/Library/Logs/DiagnosticReports -name 'horizon-client*' -newer "$APP/Info.plist" \
  -exec cp {} "$OUT/" \; 2>/dev/null
ls "$OUT"
