#!/usr/bin/env node
// Parity suite, delivery unit 1: macOS probe.
//
// Questions this answers on a hosted macOS runner, failing closed on each:
//   1. Does the embedded WebDriver plugin serve a parity build in release profile?
//   2. Is the accessibility-style tree identical across fresh launches?
//   3. Are webview screenshots identical (decoded RGBA) within a session and across launches?
//   4. Does a real OS click (CGEvent) reach the page as a trusted event and change the tree?
//   5. Does the app leave the runner's real HOME untouched when HOME is redirected?
//
//   node probe.mjs run --app <CC Switch.app> --helper <helper> --runs <n> --out <file.json>
//   node probe.mjs compare <results.json>...
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { args, scriptTimeout, pixelDiff, RECORD, screenshot, settle, sha, sleep, summarise, TARGET, TREE, webdriver } from "../common.mjs";

const PORT = 4445;
const { wd, exec } = webdriver(`http://127.0.0.1:${PORT}`);

function cleanEnv(home) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (/^(https?|all|no)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return { ...env, HOME: home, CFFIXED_USER_HOME: home, TAURI_WEBDRIVER_PORT: String(PORT), LANG: "en_US.UTF-8", TZ: "UTC" };
}

const realHomeMarks = (home) => [".cc-switch", ".claude", ".codex", ".gemini"].filter((d) => existsSync(join(home, d)));

async function oneRun(app, helper, index) {
  const exe = join(app, "Contents/MacOS", execFileSync("plutil", ["-extract", "CFBundleExecutable", "raw", join(app, "Contents/Info.plist")]).toString().trim());
  const home = mkdtempSync(join(tmpdir(), `parity-home-${index}-`));
  const realHome = process.env.HOME;
  const before = realHomeMarks(realHome);
  try { execFileSync(helper, ["move", "1", "1"]); } catch {}
  const t0 = Date.now();
  const child = spawn(exe, [], { env: cleanEnv(home), stdio: ["ignore", "pipe", "pipe"] });
  let log = "";
  child.stdout.on("data", (d) => (log += d));
  child.stderr.on("data", (d) => (log += d));
  const result = { index, failures: [] };
  try {
    for (let i = 0; ; i++) {
      try { await wd("GET", "/status"); break; } catch { if (i > 200) throw new Error("WebDriver server never answered"); await sleep(300); }
    }
    result.serverUpMs = Date.now() - t0;
    const session = await wd("POST", "/session", { capabilities: {} });
    const sid = session.sessionId;
    const handles = await wd("GET", `/session/${sid}/window/handles`).catch(() => []);
    result.windowHandles = handles;
    if (handles.includes("main")) await wd("POST", `/session/${sid}/window`, { handle: "main" });
    result.scriptTimeoutSet = await scriptTimeout(wd, sid);

    result.settleErrors = [];
    const tree = await settle(exec, sid, 30000, result.settleErrors);
    if (result.settleErrors.length) result.failures.push(`settled only after ${result.settleErrors.length} failed script call(s), first: ${result.settleErrors[0]}`);
    result.settledMs = Date.now() - t0;
    result.treeHash = sha(tree);
    result.tree = JSON.parse(tree);

    const s1 = await screenshot(wd, sid);
    const s2 = await screenshot(wd, sid);
    result.size = [s1.img.w, s1.img.h];
    result.pixelsHash = sha(s1.img.px);
    result.inSessionPixelDiff = pixelDiff(s1.img, s2.img);
    writeFileSync(join(process.env.PARITY_OUT_DIR || ".", `shot-${index}.png`), s1.png);
    if (result.inSessionPixelDiff.differing !== 0) result.failures.push("two screenshots in one session differ");

    const target = await exec(sid, TARGET);
    result.target = target;
    if (!target) {
      result.failures.push("no clickable button found");
    } else {
      await exec(sid, RECORD);
      const helperOut = (...a) => execFileSync(helper, a.map(String)).toString().trim();
      const [wx, wy] = helperOut("bounds", child.pid).split(/\s+/).map(Number);
      const point = { x: wx + target.x, y: wy + target.y };
      result.click = { window: [wx, wy], point, frontmostBefore: Number(helperOut("frontmost")), appPid: child.pid };
      result.click.activate = helperOut("activate", child.pid);
      await sleep(600);
      result.click.frontmostAfterActivate = Number(helperOut("frontmost"));
      result.click.attempts = [];
      for (let attempt = 1; attempt <= 2; attempt++) {
        helperOut("click", point.x, point.y);
        await sleep(800);
        const events = await exec(sid, "return window.__parity;");
        result.click.attempts.push({ attempt, events });
        if (events.length) break;
      }
      const events = result.click.attempts.at(-1).events;
      const trusted = events.filter((e) => e.trusted);
      if (trusted.length === 0) {
        result.failures.push(events.length ? "click arrived but not as a trusted event" : "no event reached the page from two OS clicks");
      } else if (!trusted.some((e) => e.target === target.name)) {
        result.failures.push(`trusted click landed on "${trusted[0].target}", expected "${target.name}"`);
      }
      const after = await settle(exec, sid).catch(() => tree);
      result.click.treeChanged = after !== tree;
      if (!result.click.treeChanged) result.failures.push("OS click did not change the tree");
    }
    await wd("DELETE", `/session/${sid}`).catch(() => {});
  } catch (e) {
    result.failures.push(String(e.message || e));
    if (e.detail) {
      result.settleDetail = e.detail;
      // Where is the app stuck? A main-thread sample and the WebKit helper processes, while it still runs.
      const out = process.env.PARITY_OUT_DIR || ".";
      try { execFileSync("sample", [String(child.pid), "3", "-file", join(out, `sample-${index}.txt`)], { stdio: "ignore", timeout: 20000 }); } catch {}
      try { result.processes = execFileSync("ps", ["-axo", "pid,ppid,stat,etime,rss,command"]).toString().split("\n").filter((l) => /WebKit|CC Switch|cc-switch/.test(l)); } catch {}
    }
  } finally {
    try { execFileSync(helper, ["move", "1", "1"]); } catch {}
    child.kill("SIGTERM");
    const exited = await Promise.race([new Promise((r) => child.once("exit", (code, sig) => r({ code, sig }))), sleep(10000).then(() => null)]);
    if (!exited) { child.kill("SIGKILL"); result.failures.push("app did not exit within 10 s of SIGTERM"); }
    result.exit = exited;
    result.totalMs = Date.now() - t0;
    const touched = realHomeMarks(realHome).filter((d) => !before.includes(d));
    result.realHomeTouched = touched;
    if (touched.length) result.failures.push(`real HOME gained ${touched.join(", ")}`);
    result.redirectedHome = execFileSync("find", [home, "-maxdepth", "3"]).toString().split("\n").filter(Boolean).map((p) => p.slice(home.length) || "/").sort();
    if (result.failures.length) result.log = log.slice(-4000);
    rmSync(home, { recursive: true, force: true });
  }
  return result;
}

async function main() {
  const a = args(process.argv.slice(2));
  if (a._[0] === "compare") {
    const all = a._.slice(1).map((f) => JSON.parse(readFileSync(f, "utf8")));
    const runs = all.flatMap((r, i) => r.runs.map((x) => ({ ...x, index: `${i}.${x.index}` })));
    const s = summarise(runs);
    console.log(JSON.stringify({ files: a._.slice(1), axTrusted: all.map((r) => r.axTrusted), ...s }, null, 2));
    process.exit(s.pass ? 0 : 1);
  }
  if (a._[0] !== "run" || !a.app || !a.helper) {
    console.error("usage: probe.mjs run --app <.app> --helper <helper> [--runs n] [--out file] | compare <files...>");
    process.exit(2);
  }
  if (process.env.PARITY_OUT_DIR) mkdirSync(process.env.PARITY_OUT_DIR, { recursive: true });
  const axTrusted = execFileSync(a.helper, ["trusted"]).toString().trim();
  const runs = [];
  for (let i = 0; i < Number(a.runs || 3); i++) {
    const r = await oneRun(a.app, a.helper, i);
    console.log(`run ${i}: tree ${r.treeHash} pixels ${r.pixelsHash} start ${r.serverUpMs} ms settle ${r.settledMs} ms total ${r.totalMs} ms ${r.failures.length ? "FAIL " + r.failures.join("; ") : "ok"}`);
    runs.push(r);
  }
  const summary = summarise(runs);
  const out = { axTrusted, summary, runs };
  writeFileSync(a.out || "probe-results.json", JSON.stringify(out, null, 2));
  console.log(JSON.stringify({ axTrusted, ...summary }, null, 2));
  process.exit(summary.pass ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
