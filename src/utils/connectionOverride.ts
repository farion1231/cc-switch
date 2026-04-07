const IPV4_REGEX =
  /^(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.(25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/;

function isValidPort(portText: string): boolean {
  const port = Number.parseInt(portText, 10);
  return Number.isInteger(port) && port >= 1 && port <= 65535;
}

function isValidIpv6(value: string): boolean {
  // Lightweight IPv6 shape check, strict validation is enforced again in Rust backend.
  return value.includes(":");
}

export function validateConnectionOverride(value?: string): string | null {
  const input = (value ?? "").trim();
  if (!input) return null;

  if (input.startsWith("[")) {
    const closing = input.indexOf("]");
    if (closing <= 1) {
      return "IPv6 地址格式无效，应为 [IPv6]:端口";
    }
    const host = input.slice(1, closing);
    const rest = input.slice(closing + 1);
    if (!rest.startsWith(":")) {
      return "请提供端口，格式示例：[2001:db8::1]:443";
    }
    const portText = rest.slice(1);
    if (!isValidIpv6(host)) {
      return "IPv6 地址格式无效，应为 [IPv6]:端口";
    }
    if (!isValidPort(portText)) {
      return "端口必须在 1-65535 之间";
    }
    return null;
  }

  const split = input.split(":");
  if (split.length !== 2) {
    return "连接覆写地址必须是 IPv4:端口 或 [IPv6]:端口";
  }

  const [host, portText] = split;
  if (!IPV4_REGEX.test(host)) {
    return "仅支持 IPv4 或 [IPv6] 格式的连接覆写地址";
  }
  if (!isValidPort(portText)) {
    return "端口必须在 1-65535 之间";
  }
  return null;
}
