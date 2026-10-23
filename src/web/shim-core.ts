/**
 * `@tauri-apps/api/core` 的 web shim。
 *
 * `invoke(cmd, args)` 经 routes-map 派发 fetch（Bearer 鉴权），按 unwrap 规则
 * 归一化返回，使形状与 Tauri command 一致。映射命令走 REST；未映射命令
 * 返回 `null`（warn）——挂真实 `<App/>` 时非控制面命令众多，抛异常会炸渲染。
 * 控制面 39 个命令仍走 REST。`isTauri = false` —— 运行期探测据此走 web 分支。
 */

import { ROUTES, applyUnwrap } from "./routes-map";

/**
 * 本地桩：未映射且调用方不 null-guard 的命令，返回类型安全的空默认值
 * （避免 `.length` / `.map` 之类的渲染期崩溃）。控制面命令走 ROUTES，不在这里。
 */
const WEB_STUBS: Record<string, unknown> = {
  // 环境变量冲突检查（非控制面）→ 空数组/空对象
  check_env_conflicts: [],
  check_all_env_conflicts: {},

  // —— 原生文件对话框（HTTP 无原生对话框）——
  // open_file_dialog / save_file_dialog 由 WEB_OVERRIDES 接管（SQL 导出/导入）
  open_zip_file_dialog: null,
  pick_directory: null,

  // —— shell 打开 / 外部应用（Web 无系统访问权限）——

  open_config_folder: false,
  open_app_config_folder: false,
  open_hermes_web_ui: null,
  open_workspace_directory: false,
  launch_session_terminal: false,
  open_provider_terminal: false,

  // —— 应用生命周期（Web 无法重启/更新安装）——
  restart_app: false,
  install_update_and_restart: false,
  check_app_update_available: null, // Option<String> → null = 无更新
  check_for_updates: false,
  set_app_config_dir_override: false,
  get_app_config_dir_override: null, // Option<String> → null = 无 override

  // —— 窗口/UI 控制（Web 无窗口）——
  set_window_theme: null,
  enter_lightweight_mode: null,
  exit_lightweight_mode: null,

  // —— AppHandle 事件耦合的用量查询（需 Tauri emit）——
  queryProviderUsage: null,

  // —— 浏览器 API（不能走服务器）——
  copy_text_to_clipboard: null, // navigator.clipboard.writeText() 在前端处理
};

/**
 * 未映射命令的类型推断默认值：默认 `[]`（最抗崩——`.length`/`.map`/`.filter`
 * 都安全，属性访问返回 undefined 一般也无害）。仅以下例外返回标量：
 * - `is_`/`has_`/`can_`/`should_` 前缀 → false（布尔）
 * - `_dir`/`_path`/`_url`/`_id`/`_name`/`_version`/`_key`/`_token` 后缀 → ""（字符串，防 `.split()` 崩）
 */

function token(): string {
  try {
    return localStorage.getItem("ccs.restToken") ?? "";
  } catch {
    return "";
  }
}

let redirectingToLogin = false;
/** 401 时:清旧 token + 回登录页(TokenGate) */
function onUnauthorized(cmd: string): never {
  try {
    localStorage.removeItem("ccs.restToken");
  } catch {}
  if (!redirectingToLogin) {
    redirectingToLogin = true;
    // 导航到无 token 的干净 URL,触发 TokenGate 登录页
    location.href = location.pathname;
  }
  throw new Error(`Unauthorized: REST token missing/invalid (command ${cmd})`);
}

export const isTauri = false;

// —— Web 专用覆盖：SQL 导出/导入（绕过原生文件对话框）——
// 桌面端用 save_file_dialog / open_file_dialog + export_config_to_file / import_config_from_file。
// Web 端无原生对话框，改为：导出 → REST blob 下载；导入 → <input type=file> 上传。
let pendingImport: string | null = null;

function pickImportFile(): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".sql,text/sql,text/plain";
    input.addEventListener("cancel", () => resolve(null));
    input.addEventListener("change", () => {
      const f = input.files?.[0];
      if (!f) {
        resolve(null);
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        pendingImport = reader.result as string;
        resolve(f.name);
      };
      reader.onerror = () => resolve(null);
      reader.readAsText(f);
    });
    input.click();
  });
}

function consumePendingImport(): string | null {
  const v = pendingImport;
  pendingImport = null;
  return v;
}

const WEB_OVERRIDES: Record<
  string,
  (args: Record<string, unknown>) => Promise<unknown>
> = {
  // 导出：save_file_dialog 返回 dummy 路径让流程继续
  save_file_dialog: async (args) => {
    return (args.defaultName as string) ?? "cc-switch-export.sql";
  },
  // export_config_to_file → 直接下载 SQL blob
  export_config_to_file: async () => {
    const resp = await fetch("/control/v1/db/export-sql", {
      headers: { Authorization: `Bearer ${token()}` } as Record<string, string>,
    });
    if (!resp.ok) throw new Error(`export-sql → ${resp.status}`);
    const blob = await resp.blob();
    const cd = resp.headers.get("content-disposition") ?? "";
    const m = cd.match(/filename="?([^"]+)"?/);
    const filename = m?.[1] ?? "cc-switch-export.sql";
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
    return { success: true, message: "", filePath: filename };
  },
  // 导入：open_file_dialog → 弹文件选择器 + 读内容
  open_file_dialog: async () => {
    return pickImportFile();
  },
  // import_config_from_file → POST 存储的 SQL 内容
  import_config_from_file: async () => {
    const sql = consumePendingImport();
    if (sql === null) {
      return { success: false, message: "No file selected", backupId: null };
    }
    const resp = await fetch("/control/v1/db/import-sql", {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token()}`,
        "Content-Type": "text/plain",
      },
      body: sql,
    });
    if (!resp.ok) throw new Error(`import-sql → ${resp.status}`);
    return await resp.json();
  },
  // open_external → 浏览器新标签打开（关于页/GitHub/官网/OAuth 验证等）
  open_external: async (args) => {
    const url = args.url as string;
    if (url && /^https?:\/\//.test(url)) {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  },
};

// —— 路由发现：lazy-fetch GET /control/v1/__routes（宏 inventory dump）——
// 启动时不需手维护 routes-map；迁到 #[command_api] 的命令自动出现在表中，
// shim 据此构造 fetch（path 参填 URL，body/query 从 args）。routes-map 仍作
// routes-map 权威优先（unwrap 覆盖 / WEB_STUBS），未在表中也不在发现表 → reject。
export type RouteMeta = {
  cmd: string;
  method: string;
  path: string;
  pathParams: string[];
};
let autoRoutes: Map<string, RouteMeta> | null = null;

/** @internal 测试注入路由表（绕过 lazy-fetch）。 */
export function __setAutoRoutesForTest(m: Map<string, RouteMeta> | null): void {
  autoRoutes = m;
}
async function ensureAutoRoutes(): Promise<Map<string, RouteMeta>> {
  if (autoRoutes && autoRoutes.size > 0) return autoRoutes;
  const m = new Map<string, RouteMeta>();
  try {
    const resp = await fetch("/control/v1/__routes", {
      headers: { Authorization: `Bearer ${token()}` } as Record<string, string>,
    });
    if (resp.ok) {
      const arr = (await resp.json()) as unknown;
      if (Array.isArray(arr)) {
        for (const e of arr as RouteMeta[]) {
          if (e && typeof e.cmd === "string") m.set(e.cmd, e);
        }
      }
    }
  } catch {
    // 网络错误 / 非 JSON → 空表
  }
  autoRoutes = m;
  return m;
}

function autoFetch(
  meta: RouteMeta,
  args: Record<string, unknown>,
): { url: string; init: RequestInit } {
  let path = meta.path;
  const body = { ...args };
  for (const p of meta.pathParams) {
    // Try snake_case first, then camelCase fallback (frontend sends camelCase)
    const val = args[p] ?? args[toCamelCase(p)] ?? "";
    path = path.replace(`:${p}`, encodeURIComponent(String(val)));
    delete body[p];
    delete body[toCamelCase(p)];
  }
  const init: RequestInit = {
    method: meta.method,
    headers: { Authorization: `Bearer ${token()}` } as Record<string, string>,
  };
  // Query params go in the URL for ALL methods (not just GET)
  const qs = new URLSearchParams();
  for (const [k, v] of Object.entries(body)) {
    if (v == null) continue;
    if (Array.isArray(v)) {
      // Arrays: send as repeated query params for GET, JSON body for non-GET
      if (meta.method === "GET") {
        for (const item of v) qs.append(k, String(item));
      }
      continue;
    }
    if (typeof v === "object") continue;
    qs.set(k, String(v));
  }
  const s = qs.toString();
  if (s) path += `?${s}`;

  // For non-GET: put remaining object/array args in JSON body
  const bodyObj: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(body)) {
    if (v == null) continue;
    if (typeof v === "object") {
      bodyObj[k] = v;
      delete body[k];
    }
  }
  if (meta.method !== "GET" && Object.keys(bodyObj).length > 0) {
    (init.headers as Record<string, string>)["Content-Type"] =
      "application/json";
    init.body = JSON.stringify(bodyObj);
  }
  return { url: path, init };
}

/** Convert snake_case to camelCase */
function toCamelCase(s: string): string {
  return s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());
}

export async function invoke<T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const override = WEB_OVERRIDES[cmd];
  if (override) return (await override(args ?? {})) as T;
  const r = ROUTES[cmd];
  if (!r) {
    if (cmd in WEB_STUBS) return WEB_STUBS[cmd] as T;
    // routes-map 权威未命中 → 查路由发现表（宏自动 dump）。
    // 命中 → 据元数据构造 fetch（passthrough，无 unwrap）；未命中 → reject。
    const table = await ensureAutoRoutes();
    const meta = table.get(cmd);
    if (meta) {
      const { url, init } = autoFetch(meta, args ?? {});
      let resp: Response;
      try {
        resp = await fetch(url, init);
      } catch (e) {
        throw new Error(`NetworkError: ${cmd}: ${(e as Error).message}`);
      }
      if (resp.status === 401) {
        onUnauthorized(cmd);
      }
      if (!resp.ok) {
        const text = await resp.text().catch(() => "");
        throw new Error(`REST ${cmd} → ${resp.status}: ${text}`);
      }
      return (await resp.json()) as T;
    }
    // 未映射且未在发现表 → reject（不静默降级）
    throw new Error(
      `[web-shim] unmapped command: "${cmd}" — no REST route available`,
    );
  }
  const url = r.path(args ?? {});
  const init: RequestInit = {
    method: r.method,
    headers: {
      Authorization: `Bearer ${token()}`,
    } as Record<string, string>,
  };
  if (r.method !== "GET") {
    const body = r.body ? r.body(args ?? {}) : (args ?? {});
    (init.headers as Record<string, string>)["Content-Type"] =
      "application/json";
    init.body = JSON.stringify(body);
  }
  let resp: Response;
  try {
    resp = await fetch(url, init);
  } catch (e) {
    throw new Error(`NetworkError: ${cmd}: ${(e as Error).message}`);
  }
  if (resp.status === 401) {
    onUnauthorized(cmd);
  }
  if (!resp.ok) {
    const text = await resp.text().catch(() => "");
    throw new Error(`REST ${cmd} → ${resp.status}: ${text}`);
  }
  const data = await resp.json();
  return applyUnwrap(data, r.unwrap) as T;
}

// Tauri core 还导出 convertFileSrc / Resource / Channel 等，web 下无意义；
// 插件（updater 等）可能 import 它们，给最小桩避免构建失败。
export function convertFileSrc(v: string): string {
  return v;
}

export class Resource {
  rid = 0;
  constructor(rid?: number) {
    this.rid = rid ?? 0;
  }
  async close(): Promise<void> {}
}

export class Channel<T = unknown> {
  onmessage: ((response: T) => void) | null = null;
  postMessage(_message: T): void {}
  toJSON() {
    return { __tauriChannelMarker__: true };
  }
}

export const transformCallback = (_cb: () => void): number => 0;
