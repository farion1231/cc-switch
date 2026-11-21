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
