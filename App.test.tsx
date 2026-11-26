// The panel as a desktop app (its main window and the popovers beside
// it). Run with MIHOBAR_HOME set to an empty directory, so the tests never
// touch the real data directory, and MIHOBAR_SHOW_PANEL=1, which shows the
// panel at launch (no test can click the menu bar item).
import { expect, open, test } from "nsh/test";

// Connections, usage, logs and rules are in the advanced window (in a
// bundle a process of its own; here a window of the panel's). The panel is
// pinned first, so it stays while that window has the focus. The test
// runner finds a second window's texts and fields but not its icon-only
// controls (the window's sidebar), so these tests see its first page; the
// pages are checked in Rust (usage, conns) and by hand.
async function manage(app) {
  await app.press("固定").catch(() => {});
  await app.press("仪表盘");
}

// Stopped: the header's start/stop button offers to start again.
const stopped = (app) => app.waitUntil(() => app.count("启动内核") > 0, { timeout: 10000 });

test("shows the panel's sections", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  const texts = app.texts();
  for (const label of ["MihoBar", "代理模式", "节点", "设置", "仪表盘"]) {
    expect(texts).toContain(label);
  }
  // The proxy tab: quick rows and the (still empty) groups.
  expect(texts).toContain("配置");
  expect(texts).toContain("Mihobar");
  expect(texts).not.toContain("Mihobar.yaml");
  expect(texts).toContain("暂无代理组");
  // The terminal command is a setting of the ports, not a quick row.
  expect(texts).not.toContain("终端代理");
});

test("switches tabs and opens the advanced window", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("设置");
  await app.waitFor("基础设置");
  await app.press("节点");
  expect(app.texts()).toContain("配置");
  await manage(app);
  // The connections page: its tabs and search.
  await app.waitFor("搜索 | 正则");
  await app.waitFor("活跃 0");
});

test("starts, switches mode and stops the core", async () => {
  // The app talks to its core over the real loopback network.
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  await app.press("启动内核 >> nth=1");
  // The footer shows the core's version once its API answers.
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  // The proxy mode is a segmented control: the core's mode reads as
  // selected once the core answers with it.
  const chosen = async (label) => {
    for (let i = 0; i < 50; i++) {
      const a = await app.accessibility(label);
      if (a && a.selected) return;
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error(`${label} never read as chosen`);
  };
  await chosen("规则");
  await app.press("全局");
  await chosen("全局");
  await app.press("规则");
  await chosen("规则");
  await app.press("停止");
  await stopped(app);
}, 60000);

test("lists proxy groups and switches a node", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("🧩 分组选择", { timeout: 30000 });
  expect(app.texts()).toContain("🐟 漏网之鱼");
  // A group's nodes open beside the panel when the pointer rests on it,
  // as a submenu does.
  await app.hover("🧩 分组选择");
  await app.waitFor("DIRECT", { timeout: 5000 });
  await app.press("DIRECT");
  // The group now shows DIRECT as its node (read back from the core).
  await app.waitUntil(() => {
    const texts = app.texts();
    return texts[texts.indexOf("🧩 分组选择") + 1] === "DIRECT";
  }, { timeout: 5000 });
  await app.press("停止");
  await stopped(app);
}, 60000);

test("opens the config menu", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("配置");
  await app.waitFor("本地配置", { timeout: 5000 });
  expect(app.texts()).toContain("重载配置文件");
  expect(app.texts()).toContain("导入本地配置");
});

// The config menu is a submenu like a group's nodes: it opens as the
// pointer rests on its row, closes on another row, and gives way to a
// group's nodes.
test("opens the config menu on hover", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("🧩 分组选择", { timeout: 30000 });
  await app.hover("配置");
  await app.waitFor("本地配置", { timeout: 3000 });
  await app.hover("系统代理");
  await app.waitUntil(() => !app.texts().includes("本地配置"), { timeout: 3000 });
  await app.hover("配置");
  await app.waitFor("本地配置", { timeout: 3000 });
  await app.hover("🧩 分组选择");
  await app.waitUntil(() => !app.texts().includes("本地配置") && app.texts().includes("DIRECT"), { timeout: 3000 });
  await app.press("停止");
}, 60000);

test("toggles the groups' sorting, history and hidden groups", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("节点按延迟排序");
  await app.waitFor("节点默认排序");
  await app.press("显示最近 4 次延迟");
  await app.waitFor("只显示最新延迟");
  await app.press("显示 hidden 代理组");
  await app.waitFor("隐藏 hidden 代理组");
  // Back as they were (the settings persist).
  await app.press("节点默认排序");
  await app.press("只显示最新延迟");
  await app.press("隐藏 hidden 代理组");
  await app.waitFor("节点按延迟排序");
});

test("lists connections", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await manage(app);
  // The core's own downloads (rule sets) go through it, listed live.
  await app.waitFor("搜索 | 正则", { timeout: 5000 });
  await app.press("停止");
  await stopped(app);
}, 60000);

test("shows the settings and switches the language", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("设置");
  for (const label of ["基础设置", "内核设置", "代理端口", "系统维护", "开机自启", "内核自启", "日志级别", "代理绕过", "允许局域网", "IPv6", "TCP 并发", "混合端口", "终端代理", "清理 DNS 缓存", "更新 Geo 数据库"]) {
    await app.waitFor(label);
  }
  // The language follows the system's (Chinese here) until one is chosen.
  await app.press("跟随系统 >> nth=0");
  await app.chooseMenuItem("English");
  await app.waitFor("Basic Settings");
  await app.press("English");
  await app.chooseMenuItem("Follow System");
  await app.waitFor("基础设置");
});

test("ticks automatic updates for the app and the core apart", async () => {
  const app = await open(".", { platform: "macos", locale: "en-US" });
  await app.press("Setting");
  await app.waitFor("Auto Update");
  // Both on at first: a menu ticks each apart.
  await app.press("App, Core");
  await app.chooseMenuItem("Core");
  await app.waitFor("App");
  await app.press("App");
  await app.chooseMenuItem("App");
  await app.waitFor("Off");
  await app.press("Off");
  await app.chooseMenuItem("Core");
  await app.waitFor("Core");
});

test("follows an English system and shows the speed in the menu bar", async () => {
  const app = await open(".", { platform: "macos", locale: "en-US" });
  await app.press("Setting");
  await app.waitFor("Follow System");
  await app.waitFor("Icon + Speed");
});

test("applies core settings and lays them over a restart", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await app.press("固定");
  await app.press("设置");
  await app.press("允许局域网");
  await app.waitFor("allow-lan 已保存", { timeout: 5000 });
  // A port saves itself 750 ms after the last key.
  await app.clear("text:7897");
  await app.type("text:0 >> nth=2", "7898");
  await app.waitFor("代理端口已自动保存", { timeout: 5000 });
  // A restart lays the kept settings over the config (the core answers
  // again with them).
  await app.press("重启内核");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await app.press("清理 DNS 缓存");
  // Back as it was.
  await app.press("允许局域网");
  await app.clear("text:7898");
  await app.type("text:0 >> nth=2", "7897");
  await app.waitFor("代理端口已自动保存", { timeout: 5000 });
  await app.press("停止");
  await stopped(app);
}, 90000);

test("shows the system proxy and TUN rows; the proxy needs the bundled helper", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  // The TUN stack is a menu showing the one TUN turns on with.
  for (const label of ["系统代理", "TUN 模式", "mixed"]) {
    await app.waitFor(label);
  }
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  // Outside an app bundle there is no helper to install: nothing changes,
  // the row says why.
  await app.press("系统代理");
  await app.waitFor("App 包内缺少 Helper", { timeout: 10000 });
  await app.press("停止");
  await stopped(app);
}, 60000);

test("edits the system proxy bypass list", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("设置");
  await app.press("代理绕过");
  await app.waitFor("localhost");
  await app.type("text:主机、域名、CIDR 或 <local>", "example.com");
  await app.press("新增");
  await app.waitFor("example.com");
  await app.press("恢复默认");
  await app.waitFor("<local>");
  expect(app.texts()).not.toContain("example.com");
});

// Serves the subscription the test imports: run with a web server for
// scratch files on 127.0.0.1:8765 (see README).
test("imports, lists and deletes a subscription", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN", network: "live" });
  app.onDialog((d) => (d.buttons.includes("删除") ? "删除" : d.buttons[0] ?? null));
  await app.press("配置");
  await app.press("导入订阅链接");
  await app.type("text:https://example.com/config.yaml", "http://127.0.0.1:8765/sub.yaml");
  await app.press("自动更新");
  await app.press("导入");
  await app.waitUntil(() => app.dialogs().length > 0, { timeout: 10000 });
  expect(app.dialogs()[0].title).toBe("订阅导入成功");
  await app.press("配置");
  await app.waitFor("远程配置");
  // The profile shows without its extension.
  await app.waitFor("sub");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("自动 ")));
  await app.rightClick("sub");
  await app.chooseMenuItem("删除");
  await app.waitUntil(() => app.dialogs().length > 1, { timeout: 5000 });
  await app.press("配置");
  await app.waitFor("Mihobar");
  expect(app.texts()).not.toContain("sub");
}, 60000);

test("shows the Wi-Fi auto-switch in the config menu", async () => {
  const app = await open(".", { platform: "macos", locale: "zh-Hans-CN" });
  await app.press("配置");
  await app.waitFor("Wi-Fi 自动切换", { timeout: 5000 });
});

