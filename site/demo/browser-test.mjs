import assert from "node:assert/strict";
import { chromium } from "../../target/site-qa/node_modules/playwright-core/index.mjs";

const url = process.env.CM_DEMO_URL || "http://127.0.0.1:8766/";
const browser = await chromium.launch({executablePath: process.env.CM_DEMO_CHROME});
const page = await browser.newPage({viewport: {width: 1440, height: 1000}});
const unexpected = [];
const errors = [];

page.on("pageerror", (error) => errors.push(error.message));
await page.route("**/*", (route) => {
  const request = route.request();
  if (new URL(request.url()).origin === new URL(url).origin && ["GET", "HEAD"].includes(request.method()) &&
      !/\/(?:guest|api)\/|\.(?:wasm|bin|gz)$/.test(new URL(request.url()).pathname)) {
    return route.continue();
  }
  unexpected.push([request.method(), request.url()]);
  return route.abort();
});

try {
  await page.goto(url);
  await page.locator("#replay-demo").waitFor();
  assert.deepEqual(await page.locator(".shell-lane h2").allTextContents(), ["Bash", "fish", "zsh"]);
  assert.equal(await page.locator('[data-mode="stock"] pre:visible').count(), 3);
  assert.equal(await page.locator('[data-mode="enabled"] pre:visible').count(), 3);
  assert.equal(await page.locator("#recorded-command").textContent(), "tmux a -t work:");
  assert.deepEqual(await page.locator("#recorded-keys kbd.pressed").allTextContents(), ["Tab", "Tab"]);
  assert.match(await page.locator('[data-shell="zsh"] [data-mode="enabled"] pre').textContent(), /logs/);

  await page.context().setOffline(true);
  const now = new Date("2026-01-01T00:00:00Z");
  await page.clock.install({time: now});
  await page.clock.pauseAt(new Date(now.getTime() + 1000));
  await page.locator("#replay").evaluate((button) => button.click());
  assert.equal(await page.locator("#recorded-keys kbd.pressed").count(), 0);
  await page.clock.runFor(320);
  assert.equal(await page.locator("#recorded-keys kbd.pressed").count(), 1);
  await page.clock.runFor(400);
  assert.equal(await page.locator("#recorded-keys kbd.pressed").count(), 2);
  await page.clock.resume();

  await page.getByRole("button", {name: "Python options", exact: true}).click();
  for (const shell of ["bash", "fish", "zsh"]) {
    assert.match(await page.locator(`[data-shell="${shell}"] [data-mode="enabled"] pre`).textContent(), /--format/);
    assert.doesNotMatch(await page.locator(`[data-shell="${shell}"] [data-mode="stock"] pre`).textContent(), /--format/);
  }
  await page.getByRole("button", {name: "Directories", exact: true}).click();
  assert.ok((await page.locator('[data-mode="stock"] pre').allTextContents()).every((output) => output.includes("projects")));
  await page.getByRole("button", {name: "Calculator", exact: true}).click();
  assert.ok((await page.locator('[data-mode="enabled"] pre').allTextContents()).every((output) => output.includes("2*34=68")));

  await page.setViewportSize({width: 390, height: 844});
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await page.emulateMedia({reducedMotion: "reduce"});
  await page.getByRole("button", {name: "Snippets", exact: true}).click();
  await page.locator("#replay").click();
  assert.deepEqual(await page.locator("#recorded-keys kbd.pressed").allTextContents(), ["Alt+J"]);
  assert.equal(await page.locator("#shell-grid").getAttribute("data-frame"), "1");

  assert.deepEqual(unexpected, []);
  assert.deepEqual(errors, []);
  console.log("Recorded shell comparisons, synchronized Tab replay, offline switching, reduced motion, and 390 px layout passed");
} finally {
  await browser.close();
}
