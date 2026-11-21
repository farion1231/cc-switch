// One running app and the platform input that reaches it. The verb engine (verbs.ts) only talks
// to this interface, so each OS supplies one implementation.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";
import { PROBE_SCRIPT, type ProbeCmd } from "./probe.ts";
import { ELEMENT_KEY, WebDriver } from "./webdriver.ts";

export interface AppSession {
  /** Runs one probe command in the main window's page. */
  probe<T>(cmd: ProbeCmd): Promise<T>;
  /** A trusted click at a point in viewport CSS pixels. */
  click(x: number, y: number): Promise<void>;
  /** Types into the element the last resolve returned, as key events. */
  type(element: unknown, text: string): Promise<void>;
  /** Presses a chord such as ["Control", "a"]. */
  chord(keys: readonly string[]): Promise<void>;
  /** The platform's modifier for `Mod`. */
  readonly mod: string;
  /** Requests a public URL from the main window's page: "open", or why it could not leave. */
  egress(url: string): Promise<string>;
  close(): Promise<{ log: string }>;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// W3C key codes for the named keys a chord may use.
const KEYS: Record<string, string> = {
  Control: "\uE009", Shift: "\uE008", Alt: "\uE00A", Meta: "\uE03D", Enter: "\uE007", Escape: "\uE00C",
  Tab: "\uE004", Backspace: "\uE003", Delete: "\uE017", ArrowUp: "\uE013", ArrowDown: "\uE015",
  ArrowLeft: "\uE012", ArrowRight: "\uE014", Space: " ",
};
export const keyValue = (k: string) => KEYS[k] ?? k;

/** Environment for the app: the given HOME and XDG dirs, and no proxy variables. */
export function appEnv(home: string, extra: Record<string, string> = {}): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (v === undefined || /^(https?|all|no|ftp)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return {
    ...env,
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CACHE_HOME: join(home, ".cache"),
    LANG: "en_US.UTF-8",
    TZ: "UTC",
    ...extra,
  };
}

/** Waits for the app's own document, retrying evaluations that the driver loses during startup. */
async function waitReady(wd: WebDriver, timeoutMs = 30000): Promise<void> {
  const start = Date.now();
  await wd.setScriptTimeout(1000);
  try {
    while (Date.now() - start < timeoutMs) {
      const state = await wd.exec<[string, string]>("return [document.readyState, location.href];").catch(() => null);
      if (state && state[0] === "complete" && state[1] !== "about:blank") return;
      await sleep(200);
    }
  } finally {
    await wd.setScriptTimeout(5000);
  }
  throw new Error(`the app's document did not load within ${timeoutMs} ms`);
}

// The script timeout after waitReady is 5 s; the page gives up first, so the answer always arrives.
const EGRESS_SCRIPT = `const done = arguments[arguments.length - 1];
const timer = new Promise((r) => setTimeout(() => r("no answer within 4 s"), 4000));
const req = fetch(arguments[0], { cache: "no-store", mode: "no-cors" }).then(() => "open", (e) => "refused: " + e);
Promise.race([req, timer]).then(done);`;

/** Linux: tauri-driver in front of WebKitWebDriver, under Xvfb and a D-Bus session. */
export function launchLinux(opts: { binary: string; home: string; port?: number }): Promise<AppSession> {
  return launchThroughTauriDriver({
    driver: "tauri-driver",
    nativeDriver: "/usr/bin/WebKitWebDriver",
    binary: opts.binary,
    port: opts.port ?? 4444,
    env: appEnv(opts.home, { TAURI_WEBVIEW_AUTOMATION: "true" }),
    kill: async () => {
      try { execFileSync("pkill", ["-x", "cc-switch"]); } catch {}
      for (let i = 0; i < 25; i++) {
        try { execFileSync("pgrep", ["-x", "cc-switch"]); } catch { break; }
        await sleep(200);
      }
    },
  });
}

/**
 * Windows: tauri-driver in front of msedgedriver (WebView2). There is no HOME redirection: the
 * disposable VM's profile is HOME and the app runs exactly as shipped.
 */
export function launchWindows(opts: { binary: string; driver: string; edgeDriver: string; port?: number }): Promise<AppSession> {
  const env: Record<string, string> = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (v === undefined || /^(https?|all|no|ftp)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return launchThroughTauriDriver({
    driver: opts.driver,
    nativeDriver: opts.edgeDriver,
    binary: opts.binary,
    port: opts.port ?? 4444,
    env,
    kill: async () => {
      for (const image of ["cc-switch.exe", "msedgedriver.exe"]) {
        try { execFileSync("taskkill", ["/F", "/T", "/IM", image], { stdio: "ignore" }); } catch {}
      }
      await sleep(1000);
    },
  });
}

/**
 * macOS: the parity build embeds a WebDriver server (tauri-plugin-wdio-webdriver), which locates
 * and reads. Input is real: the helper posts CGEvents at the HID tap after activating the app.
 */
export async function launchMacos(opts: { app: string; helper: string; home: string; port?: number }): Promise<AppSession> {
  const port = opts.port ?? 4445;
  const plist = join(opts.app, "Contents/Info.plist");
  const exe = join(opts.app, "Contents/MacOS", execFileSync("plutil", ["-extract", "CFBundleExecutable", "raw", plist]).toString().trim());
  const child = spawn(exe, [], {
    env: appEnv(opts.home, { CFFIXED_USER_HOME: opts.home, TAURI_WEBDRIVER_PORT: String(port) }),
    stdio: ["ignore", "pipe", "pipe"],
  });
  let log = "";
  child.stdout?.on("data", (d) => (log += d));
  child.stderr?.on("data", (d) => (log += d));
  const helper = (...a: (string | number)[]) => execFileSync(opts.helper, a.map(String)).toString().trim();
  const wd = new WebDriver(`http://127.0.0.1:${port}`);
  const close = async () => {
    await wd.deleteSession();
    // How the app ended, so a crash at quit shows in the run's log.
    const ended = child.exitCode !== null || child.signalCode !== null ? "before quit: " : "";
    const t0 = Date.now();
    child.kill("SIGTERM");
    const exited = ended || (await Promise.race([new Promise((r) => child.once("exit", () => r(true))), sleep(10000).then(() => false)]));
    if (!exited) child.kill("SIGKILL");
    try { helper("move", 1, 1); } catch {}
    const how = `\n[parity] app ended ${ended}${exited ? `code ${child.exitCode} signal ${child.signalCode} after ${Date.now() - t0} ms` : "only after SIGKILL"}`;
    return { log: log.slice(-8000) + how };
  };
  try {
    for (let i = 0; ; i++) {
      try { await wd.call("GET", "/status", undefined, 2000); break; } catch { if (i > 150) throw new Error("the embedded WebDriver server did not start"); await sleep(200); }
    }
    await wd.newSession({});
    const handles = await wd.call<string[]>("GET", `/session/${wd.sid}/window/handles`);
    if (handles.includes("main")) await wd.call("POST", `/session/${wd.sid}/window`, { handle: "main" });
    await waitReady(wd);
  } catch (e) {
    await close();
    throw e;
  }
  // The helper's names for the chord keys the verbs use.
  const NAMES: Record<string, string> = { Meta: "cmd", Control: "ctrl", Shift: "shift", Alt: "alt", Backspace: "backspace", Enter: "return", Escape: "escape", Tab: "tab", Delete: "delete", Space: "space", ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };
  const activate = async () => {
    helper("activate", child.pid!);
    await sleep(150);
  };
  return {
    mod: "Meta",
    probe: <T>(cmd: ProbeCmd) => wd.exec<T>(PROBE_SCRIPT, [cmd]),
    egress: (url: string) => wd.execAsync<string>(EGRESS_SCRIPT, [url]),
    async click(x, y) {
      await activate();
      const [wx, wy] = helper("bounds", child.pid!).split(/\s+/).map(Number);
      helper("click", wx + x, wy + y);
    },
    async type(_element, text) {
      await activate();
      helper("type", text);
    },
    async chord(keys) {
      await activate();
      helper("key", keys.map((k) => NAMES[k] ?? k.toLowerCase()).join("+"));
    },
    close,
  };
}

async function launchThroughTauriDriver(o: {
  driver: string; nativeDriver: string; binary: string; port: number; env: Record<string, string>; kill: () => Promise<void>;
}): Promise<AppSession> {
  const { port } = o;
  const driver: ChildProcess = spawn(o.driver, ["--port", String(port), "--native-driver", o.nativeDriver], {
    env: o.env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let log = "";
  driver.stdout?.on("data", (d) => (log += d));
  driver.stderr?.on("data", (d) => (log += d));
  const wd = new WebDriver(`http://127.0.0.1:${port}`);
  const close = async () => {
    await wd.deleteSession();
    driver.kill("SIGTERM");
    await Promise.race([new Promise((r) => driver.once("exit", r)), sleep(5000)]);
    await o.kill();
    return { log: log.slice(-8000) };
  };
  try {
    for (let i = 0; ; i++) {
      try { await wd.call("GET", "/status", undefined, 2000); break; } catch { if (i > 100) throw new Error("tauri-driver did not start"); await sleep(200); }
    }
    await wd.newSession({ alwaysMatch: { "tauri:options": { application: o.binary } } });
    await waitReady(wd);
  } catch (e) {
    await close();
    throw e;
  }
  return {
    mod: "Control",
    probe: <T>(cmd: ProbeCmd) => wd.exec<T>(PROBE_SCRIPT, [cmd]),
    egress: (url: string) => wd.execAsync<string>(EGRESS_SCRIPT, [url]),
    async click(x, y) {
      await wd.actions([{
        type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
        actions: [
          { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(x), y: Math.round(y) },
          { type: "pointerDown", button: 0 },
          { type: "pointerUp", button: 0 },
        ],
      }]);
      await wd.releaseActions();
    },
    async type(element, text) {
      const id = (element as Record<string, string> | undefined)?.[ELEMENT_KEY];
      if (!id) throw new Error("no element reference to type into");
      await wd.sendKeys(id, text);
    },
    async chord(keys) {
      const down = keys.map((k) => ({ type: "keyDown", value: keyValue(k) }));
      const up = [...keys].reverse().map((k) => ({ type: "keyUp", value: keyValue(k) }));
      await wd.actions([{ type: "key", id: "keyboard", actions: [...down, ...up] }]);
      await wd.releaseActions();
    },
    close,
  };
}
