// The panel as a desktop app (its main window and the popovers beside
// it). Run with MIHOBAR_HOME set to an empty directory, so the tests never
// touch the real data directory, and MIHOBAR_SHOW_PANEL=1, which shows the
// panel at launch (no test can click the menu bar item).
import { expect, open, test } from "nsh/test";

// Rules, connections and logs are in the advanced window. The panel is
// pinned first, so it stays while that window has the focus (a pinned
// panel stays when the focus leaves).
async function manage(app, section?: string) {
  await app.press("固定").catch(() => {});
  await app.press("高级");
  if (section) await app.press(section);
}

test("shows the panel's sections", async () => {
  const app = await open(".", { platform: "macos" });
  const texts = app.texts();
  for (const label of ["Mihobar", "出站模式", "节点", "设置", "高级"]) {
    expect(texts).toContain(label);
  }
  // The proxy tab: quick rows and the (still empty) groups.
  expect(texts).toContain("切换配置");
  expect(texts).toContain("Mihobar.yaml");
  expect(texts).toContain("复制终端命令");
  expect(texts).toContain("暂无代理组");
});

test("switches tabs and opens the advanced window", async () => {
  const app = await open(".", { platform: "macos" });
  await app.press("设置");
  await app.waitFor("基础设置");
  await app.press("节点");
  expect(app.texts()).toContain("切换配置");
  await manage(app, "日志");
  // Every launch logs itself.
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("Mihobar 0.1.0 started")));
  await app.press("分流");
  await app.press("连接");
  await app.waitFor("过滤域名、IP...");
});

test("starts, switches mode and stops the core", async () => {
  // The app talks to its core over the real loopback network.
  const app = await open(".", { platform: "macos", network: "live" });
  await app.press("启动内核 >> nth=1");
  // The footer shows the core's version once its API answers.
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  // The outbound mode is a row whose submenu holds the modes; the row
  // shows the core's mode as it reads it back.
  await app.waitFor("规则", { timeout: 5000 });
  await app.press("出站模式");
  await app.chooseMenuItem("全局");
  await app.waitFor("全局", { timeout: 5000 });
  await app.press("出站模式");
  await app.chooseMenuItem("规则");
  await app.waitFor("规则", { timeout: 5000 });
  expect(app.texts()).not.toContain("全局");
  await app.press("停止");
  await manage(app, "日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("lists proxy groups and switches a node", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
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
  await manage(app, "日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("opens the config menu", async () => {
  const app = await open(".", { platform: "macos" });
  await app.press("切换配置");
  await app.waitFor("本地配置", { timeout: 5000 });
  expect(app.texts()).toContain("重载配置文件");
  expect(app.texts()).toContain("导入本地配置");
});

test("toggles the groups' sorting, history and hidden groups", async () => {
  const app = await open(".", { platform: "macos" });
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

// The management window's icon-only buttons (copy, clear, close all,
// the rules' list toggle) are not found by the test runner in a second
// window (it finds that window's texts and fields): their tests use what
// it finds.
test("filters and searches the logs", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await manage(app, "日志");
  // Sources and levels; mihomo's own lines arrive from its stream.
  for (const label of ["Mihobar", "Mihomo", "信息", "警告", "错误"]) {
    await app.waitFor(label);
  }
  await app.waitFor("[mihomo started]", { timeout: 5000 }).catch(() => {});
  // Only Mihobar's own lines.
  await app.press("Mihobar >> nth=1");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("Mihobar 0.1.0 started")), { timeout: 5000 });
  await app.press("全部 >> nth=0");
  await app.press("停止");
  await app.type("text:搜索日志...", "[mihomo stopped]");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("lists connections", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await manage(app, "连接");
  await app.waitFor("过滤域名、IP...", { timeout: 5000 });
  await app.press("停止");
  await app.press("日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("shows and filters the rules", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  await manage(app, "分流");
  await app.waitFor("规则集", { timeout: 5000 });
  // The default config ends with a MATCH rule to 🐟 漏网之鱼 (searched
  // for: the list only draws the rows on screen).
  await app.type("text:搜索目标、类型、策略...", "MATCH");
  await app.waitFor("MATCH", { timeout: 10000 });
  await app.clear("text:MATCH");
  // The type chips carry their counts ("规则集 26").
  const chip = app.texts().find((t) => /^规则集 \d+$/.test(t))!;
  await app.press(chip);
  await app.waitFor("RULESET", { timeout: 5000 });
  expect(app.texts()).not.toContain("IPCIDR");
  await app.press("停止");
  await app.press("日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("shows the settings and switches the language", async () => {
  const app = await open(".", { platform: "macos" });
  await app.press("设置");
  for (const label of ["基础设置", "内核设置", "代理端口", "系统维护", "开机自启", "内核自启", "日志级别", "代理绕过", "允许局域网", "IPv6", "TCP 并发", "混合端口", "清理 DNS 缓存", "更新 Geo 数据库"]) {
    await app.waitFor(label);
  }
  await app.press("简体中文");
  await app.chooseMenuItem("English");
  await app.waitFor("Basic Settings");
  await app.press("English");
  await app.chooseMenuItem("简体中文");
  await app.waitFor("基础设置");
});

test("applies core settings and lays them over a restart", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
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
  // A restart lays the kept settings over the config.
  await app.press("重启内核");
  await manage(app, "日志");
  await app.type("text:搜索日志...", "PATCH /configs succeeded");
  await app.waitUntil(
    // The log row shows the keys as its detail line (ports the config
    // leaves off, 0, are not laid over).
    () => app.texts().includes("[allow-lan, ipv6, log-level, mixed-port, mode, tcp-concurrent, tun]"),
    { timeout: 20000 },
  );
  await app.clear("text:PATCH /configs succeeded");
  await app.press("清理 DNS 缓存");
  await app.press("日志");
  await app.type("text:搜索日志...", "清理 DNS 缓存");
  await app.waitFor("清理 DNS 缓存: 成功", { timeout: 5000 });
  await app.clear("text:清理 DNS 缓存");
  // Back as it was.
  await app.press("允许局域网");
  await app.clear("text:7898");
  await app.type("text:0 >> nth=2", "7897");
  await app.waitFor("代理端口已自动保存", { timeout: 5000 });
  await app.press("停止");
  await app.press("日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 90000);

test("shows the system proxy and TUN rows; the proxy needs the bundled helper", async () => {
  const app = await open(".", { platform: "macos", network: "live" });
  for (const label of ["系统代理", "TUN 模式", "system", "gvisor", "mixed", "mips"]) {
    await app.waitFor(label);
  }
  await app.press("启动内核 >> nth=1");
  await app.waitFor("MIHOMO v1.19.34", { timeout: 30000 });
  // Outside an app bundle there is no helper to install: nothing changes,
  // the row says why.
  await app.press("系统代理");
  await app.waitFor("App 包内缺少 Helper", { timeout: 10000 });
  await app.press("停止");
  await manage(app, "日志");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("[mihomo stopped] exit=")), { timeout: 10000 });
}, 60000);

test("edits the system proxy bypass list", async () => {
  const app = await open(".", { platform: "macos" });
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
  const app = await open(".", { platform: "macos", network: "live" });
  app.onDialog((d) => (d.buttons.includes("删除") ? "删除" : d.buttons[0] ?? null));
  await app.press("切换配置");
  await app.press("导入订阅链接");
  await app.type("text:https://example.com/config.yaml", "http://127.0.0.1:8765/sub.yaml");
  await app.press("自动更新");
  await app.press("导入");
  await app.waitUntil(() => app.dialogs().length > 0, { timeout: 10000 });
  expect(app.dialogs()[0].title).toBe("订阅导入成功");
  await app.press("切换配置");
  await app.waitFor("远程配置");
  await app.waitFor("sub.yaml");
  await app.waitUntil(() => app.texts().some((t) => t.startsWith("自动 ")));
  await app.rightClick("sub.yaml");
  await app.chooseMenuItem("删除");
  await app.waitUntil(() => app.dialogs().length > 1, { timeout: 5000 });
  await app.press("切换配置");
  await app.waitFor("Mihobar.yaml");
  expect(app.texts()).not.toContain("sub.yaml");
}, 60000);

test("shows the Wi-Fi auto-switch in the config menu", async () => {
  const app = await open(".", { platform: "macos" });
  await app.press("切换配置");
  await app.waitFor("开启 Wi-Fi 自动切换", { timeout: 5000 });
});

