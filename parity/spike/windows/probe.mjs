#!/usr/bin/env node
// Parity suite, delivery unit 3 (first slice): Windows probe, run on a disposable hosted runner.
//
// Drives a parity build through tauri-driver and msedgedriver (WebView2). Fails closed when:
//   1. the accessibility-style tree or the webview pixels differ across fresh launches;
//   2. two screenshots in one session differ;
//   3. a W3C pointer click does not reach the page as a trusted event, or does not change the tree;
//   4. the user profile the app was not given gains .cc-switch, .claude, .codex or .gemini;
//   5. the WebView2 profile carries state from one launch into the next.
//
// Windows has no HOME redirection. The app's own paths follow CC_SWITCH_TEST_HOME, and WebView2's
// user data folder follows WEBVIEW2_USER_DATA_FOLDER. Code that asks the OS for the profile
// directly still reaches the real one, which check 4 catches.
//
//   node probe.mjs run --app <cc-switch.exe> --driver <tauri-driver.exe> --edge <msedgedriver.exe> --runs <n> --out <file.json>
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, sep } from "node:path";
import { args, FOCUS, pixelDiff, RECORD, screenshot, scriptTimeout, settle, sha, sleep, summarise, TARGET, webdriver } from "../common.mjs";

const PORT = 4444;
const { wd, exec } = webdriver(`http://127.0.0.1:${PORT}`);
const IDENTIFIER = "com.ccswitch.desktop";

function cleanEnv(home, webviewData) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (/^(https?|all|no|ftp)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return { ...env, CC_SWITCH_TEST_HOME: home, WEBVIEW2_USER_DATA_FOLDER: webviewData, LANG: "en_US.UTF-8", TZ: "UTC" };
}

// Places the app could write that are outside what it was given.
function outsideMarks() {
  const profile = process.env.USERPROFILE, local = process.env.LOCALAPPDATA, roaming = process.env.APPDATA;
  return [
    ...[".cc-switch", ".claude", ".codex", ".gemini"].map((d) => join(profile, d)),
    join(local, IDENTIFIER),
    join(roaming, IDENTIFIER),
  ].filter((p) => existsSync(p));
}

function list(dir, depth = 4) {
  return readdirSync(dir, { recursive: true })
    .map((p) => `${sep}${p}`)
    .filter((p) => p.split(sep).length - 1 <= depth)
    .map((p) => p.split(sep).join("/"))
    .sort();
}

function killApp() {
  for (const image of ["cc-switch.exe", "msedgedriver.exe"]) {
    try { execFileSync("taskkill", ["/F", "/T", "/IM", image], { stdio: "ignore" }); } catch {}
  }
}

async function oneRun(app, driverExe, edgeExe, index, outDir) {
  const home = mkdtempSync(join(tmpdir(), `parity-home-${index}-`));
  const webviewData = mkdtempSync(join(tmpdir(), `parity-webview-${index}-`));
  const before = outsideMarks();
  const t0 = Date.now();
  const driver = spawn(driverExe, ["--port", String(PORT), "--native-driver", edgeExe], {
    env: cleanEnv(home, webviewData),
    stdio: ["ignore", "pipe", "pipe"],
  });
  let log = "";
  driver.stdout.on("data", (d) => (log += d));
  driver.stderr.on("data", (d) => (log += d));
  const result = { index, failures: [] };
  const step = (m) => console.error(`[run ${index}] +${Date.now() - t0} ms ${m}`);
  let sid = null;
  try {
    for (let i = 0; ; i++) {
      try { await wd("GET", "/status"); break; } catch { if (i > 150) throw new Error("tauri-driver never answered"); await sleep(200); }
    }
    step("tauri-driver up; creating session");
    const session = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } });
    sid = session.sessionId;
    result.browserVersion = session.capabilities?.browserVersion;
    result.serverUpMs = Date.now() - t0;
    result.scriptTimeoutSet = await scriptTimeout(wd, sid);
    step("session created; waiting for the tree to settle");

    result.settleErrors = [];
    const tree = await settle(exec, sid, 30000, result.settleErrors);
    if (result.settleErrors.length) result.failures.push(`settled only after ${result.settleErrors.length} failed script call(s), first: ${result.settleErrors[0]}`);
    step("settled");
    result.settledMs = Date.now() - t0;
    result.treeHash = sha(tree);
    result.tree = JSON.parse(tree);

    // The WebView2 profile must start empty on every launch: a marker left by launch N must be
    // gone in launch N+1.
    result.carriedMarker = await exec(sid, `const m = localStorage.getItem("__parity_marker"); localStorage.setItem("__parity_marker", "launch-${index}"); return m;`);
    if (result.carriedMarker !== null) result.failures.push(`WebView2 profile carried state from ${result.carriedMarker}`);

    const focus = await exec(sid, FOCUS);
    const s1 = await screenshot(wd, sid);
    await sleep(300);
    const s2 = await screenshot(wd, sid);
    step("screenshots taken");
    if ((await exec(sid, FOCUS)) !== focus) result.failures.push("taking a screenshot moved focus");
    result.size = [s1.img.w, s1.img.h];
    result.pixelsHash = sha(s1.img.px);
    result.inSessionPixelDiff = pixelDiff(s1.img, s2.img);
    writeFileSync(join(outDir, `shot-${index}.png`), s1.png);
    if (result.inSessionPixelDiff.differing !== 0) result.failures.push("two screenshots in one session differ");

    const target = await exec(sid, TARGET);
    result.target = target;
    if (!target) {
      result.failures.push("no clickable button found");
    } else {
      await exec(sid, RECORD);
      await wd("POST", `/session/${sid}/actions`, {
        actions: [{
          type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
          actions: [
            { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(target.x), y: Math.round(target.y) },
            { type: "pointerDown", button: 0 },
            { type: "pointerUp", button: 0 },
          ],
        }],
      });
      await wd("DELETE", `/session/${sid}/actions`).catch(() => {});
      await sleep(500);
      const events = await exec(sid, "return window.__parity;");
      result.click = { events };
      const trusted = events.filter((e) => e.trusted);
      if (trusted.length === 0) {
        result.failures.push(events.length ? "click arrived but not as a trusted event" : "no event reached the page");
      } else if (!trusted.some((e) => e.target === target.name)) {
        result.failures.push(`trusted click landed on "${trusted[0].target}", expected "${target.name}"`);
      }
      const after = await settle(exec, sid).catch(() => tree);
      result.click.treeChanged = after !== tree;
      if (!result.click.treeChanged) result.failures.push("click did not change the tree");
    }
  } catch (e) {
    step(`error: ${e.message || e}`);
    result.failures.push(String(e.message || e));
    if (e.detail) result.settleDetail = e.detail;
  } finally {
    step("tearing down");
    if (sid) await wd("DELETE", `/session/${sid}`, undefined, 15000).catch((e) => step(`delete session: ${e.message}`));
    driver.kill();
    await Promise.race([new Promise((r) => driver.once("exit", r)), sleep(5000)]);
    killApp();
    await sleep(1000);
    result.totalMs = Date.now() - t0;
    const touched = outsideMarks().filter((p) => !before.includes(p));
    result.outsideTouched = touched;
    if (touched.length) result.failures.push(`wrote outside what it was given: ${touched.join(", ")}`);
    result.webviewDataUsed = readdirSync(webviewData).length > 0;
    if (!result.webviewDataUsed) result.failures.push("WebView2 did not use WEBVIEW2_USER_DATA_FOLDER");
    result.redirectedHome = list(home);
    if (result.failures.length) result.log = log.slice(-4000);
    for (const d of [home, webviewData]) {
      try { rmSync(d, { recursive: true, force: true, maxRetries: 5, retryDelay: 500 }); } catch (e) { step(`cleanup ${d}: ${e.message}`); }
    }
  }
  return result;
}

async function main() {
  const a = args(process.argv.slice(2));
  if (a._[0] !== "run" || !a.app || !a.driver || !a.edge) {
    console.error("usage: probe.mjs run --app <cc-switch.exe> --driver <tauri-driver.exe> --edge <msedgedriver.exe> [--runs n] [--out file]");
    process.exit(2);
  }
  const out = a.out || "results.json";
  const outDir = dirname(out);
  mkdirSync(outDir, { recursive: true });
  const runs = [];
  for (let i = 0; i < Number(a.runs || 3); i++) {
    const r = await oneRun(a.app, a.driver, a.edge, i, outDir);
    console.log(`run ${i}: tree ${r.treeHash} pixels ${r.pixelsHash} start ${r.serverUpMs} ms settle ${r.settledMs} ms total ${r.totalMs} ms ${r.failures.length ? "FAIL " + r.failures.join("; ") : "ok"}`);
    runs.push(r);
  }
  const summary = summarise(runs);
  writeFileSync(out, JSON.stringify({ summary, runs }, null, 2));
  console.log(JSON.stringify(summary, null, 2));
  process.exit(summary.pass ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
