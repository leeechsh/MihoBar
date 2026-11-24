# Mihobar

A native macOS menu bar client for [mihomo](https://github.com/MetaCubeX/mihomo),
written in Rust on [NSH](https://nsh.dev).

Mihobar is a port of **[ClashBar](https://github.com/Sitoi/ClashBar)** by Sitoi
(v0.3.6, GPL-3.0): it follows ClashBar's behavior and reuses its localized
strings and default config. Like ClashBar, it is licensed under the
**GNU General Public License v3.0** (see `LICENSE`).

## Build

```bash
scripts/package.sh               # .nsh/out/Mihobar.app, with the privileged helper inside
```

`scripts/package.sh` fetches the mihomo core if needed (`scripts/fetch-mihomo.sh`),
runs `nsh package --target macos`, builds the helper (`helper/`), puts it in
`Contents/Library/HelperTools` with its launchd plist, and signs both ad hoc
(`dev.mihobar.helper`, `dev.mihobar.app`; `SIGN_IDENTITY` for a Developer ID).

## System proxy and TUN

Like ClashBar, Mihobar sets the system proxy through a root helper
(`/Library/PrivilegedHelperTools/dev.mihobar.helper`, a launchd XPC service that
only answers `dev.mihobar.app`), and runs TUN with a setuid-root core. Both ask
for an administrator password the first time. To try them:

1. Quit ClashBar (its system proxy and TUN go off).
2. Copy `.nsh/out/Mihobar.app` to `/Applications` (the helper installs only from there) and open it.
3. Start the core, then switch on "系统代理" (installs the helper) or "TUN 模式" (makes the core root).

To remove the helper:

```bash
sudo launchctl bootout system/dev.mihobar.helper; sudo rm -f /Library/LaunchDaemons/dev.mihobar.helper.plist /Library/PrivilegedHelperTools/dev.mihobar.helper*
```

## Test

```bash
MIHOBAR_HOME="$(mktemp -d)" MIHOBAR_SHOW_PANEL=1 nsh test
```

`MIHOBAR_HOME` moves the data directory (default
`~/Library/Application Support/mihobar`) so tests never touch real data.
The subscription test downloads `http://127.0.0.1:8765/sub.yaml`: serve a
mihomo config under that name first, for example

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory path/to/dir-with-sub.yaml
```

The remote machine test uses the test's own core (`127.0.0.1:9097`) as the
"remote" machine.

## Status

All six milestones are done:

1. Core lifecycle, configs, panel header, mode switcher, tabs, traffic and memory streams.
2. Proxy tab: traffic overview, config menu, terminal command, proxy providers, proxy groups with their node popovers, latency tests, sorting, hidden groups.
3. Connections, logs and rules tabs; log files in `logs/`; streams opened only for what is on screen.
4. Settings tab, maintenance, core upgrade, settings laid over the config after restarts, menu bar speed styles.
5. System proxy, bypass list, TUN, the privileged helper (built and tested without root, not yet tried live).
6. Subscriptions (import, auto update, refresh, delete), network monitoring (stop when offline, start again when back), Wi-Fi auto-switch (bind configs to Wi-Fi names; needs the location permission), remote machines (header menu, manager, editor; the panel drives another mihomo controller).

Not ported yet: keyboard shortcuts, the web UI button, hover-only buttons,
copied/mode banners, refreshing all providers at startup; TUN stack chips
switch on one press (ClashBar: double-click). A Developer ID build should
also pin the helper's client requirement to its Team ID.
