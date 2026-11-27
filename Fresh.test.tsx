// A first launch: MihoBar ships no profile. scripts/test.sh runs this file
// in an empty data directory (MIHOBAR_HOME without a config folder).
import { expect, open, test } from "nsh/test";

test("asks for a profile before the core can start", async () => {
  const app = await open(".", { platform: "macos", locale: "en-US" });
  await app.waitFor("No profile yet");
  expect(app.texts()).toContain("Import Local Profile");
  expect(app.texts()).toContain("Import Subscription");
  // Nothing to start yet: both start buttons (restart, start/stop) say why.
  expect(app.count("Import a profile first")).toBe(2);
  expect(app.count("Start Core")).toBe(0);
  // The subscription form opens beside the panel.
  await app.press("Import Subscription");
  await app.waitFor("Import", { timeout: 5000 });
});
