#!/usr/bin/env bash
# Builds .nsh/out/Mihobar.app with its privileged helper inside
# (Contents/Library/HelperTools and LaunchDaemons, as ClashBar's bundle
# carries com.clashbar.helper), signed ad hoc with the identifiers the
# helper and the app check: dev.mihobar.app and dev.mihobar.helper.
# Set SIGN_IDENTITY to a Developer ID to sign for distribution.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IDENTITY="${SIGN_IDENTITY:--}"
APP="$ROOT/.nsh/out/Mihobar.app"

[ -f "$ROOT/assets/bin/mihomo.gz" ] || "$ROOT/scripts/fetch-mihomo.sh"

(cd "$ROOT" && nsh package --target macos)
(cd "$ROOT/helper" && cargo build --release)

HELPER_DIR="$APP/Contents/Library/HelperTools"
PLIST_DIR="$APP/Contents/Library/LaunchDaemons"
mkdir -p "$HELPER_DIR" "$PLIST_DIR"
cp "$ROOT/helper/target/release/mihobar-helper" "$HELPER_DIR/dev.mihobar.helper"
# The launchd job (as helper/src/protocol.rs's `launchd_plist`).
cat > "$PLIST_DIR/dev.mihobar.helper.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.mihobar.helper</string>
    <key>MachServices</key>
    <dict>
        <key>dev.mihobar.helper</key>
        <true/>
    </dict>
    <key>Program</key>
    <string>/Library/PrivilegedHelperTools/dev.mihobar.helper</string>
</dict>
</plist>
PLIST

# Wi-Fi auto-switch reads the Wi-Fi name, which needs the location
# permission and its reason (as ClashBar's Info.plist has).
# nsh-extra's location feature adds a generic NSLocationUsageDescription:
# both keys say what Mihobar uses it for.
for key in NSLocationWhenInUseUsageDescription NSLocationUsageDescription; do
  /usr/libexec/PlistBuddy -c "Delete :$key" "$APP/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :$key string 'Mihobar uses your current Wi-Fi name to switch proxy config profiles automatically.'" "$APP/Contents/Info.plist"
done

codesign --force --sign "$IDENTITY" --identifier dev.mihobar.helper --options runtime "$HELPER_DIR/dev.mihobar.helper"
codesign --force --sign "$IDENTITY" --identifier dev.mihobar.app "$APP"
codesign --verify --deep --strict "$APP"
echo "packaged $APP"
