const writeErrorLog = vi.hoisted(() =>
  vi.fn<(message: string, options: { file: string }) => Promise<void>>(() =>
    Promise.resolve(),
  ),
);

vi.mock("@tauri-apps/plugin-log", () => ({
  error: writeErrorLog,
}));

import {
  installGlobalErrorHandlers,
  redactFrontendLogText,
  reportFrontendError,
} from "@/lib/frontendLogger";

describe("frontendLogger", () => {
  beforeEach(() => {
    writeErrorLog.mockClear();
  });

  it("redacts URL parameters and named credentials", () => {
    const redacted = redactFrontendLogText(
      "https://example.test/path?apiKey=query-secret&name=alice\n" +
        'api_key: "config secret with spaces"\n' +
        "Authorization: Bearer bearer-secret\n" +
        "Authorization: Basic dXNlcjpwYXNz\n" +
        "Authorization: Token token-secret\n" +
        "  Cookie: session=cookie-secret; preference=private\n" +
        "ApiKey standalone-secret\n" +
        "https://user:password@example.test/private",
    );

    expect(redacted).not.toContain("query-secret");
    expect(redacted).not.toContain("alice");
    expect(redacted).not.toContain("config secret with spaces");
    expect(redacted).not.toContain("secret with spaces");
    expect(redacted).not.toContain("bearer-secret");
    expect(redacted).not.toContain("dXNlcjpwYXNz");
    expect(redacted).not.toContain("token-secret");
    expect(redacted).not.toContain("standalone-secret");
    expect(redacted).not.toContain("cookie-secret");
    expect(redacted).not.toContain("preference=private");
    expect(redacted).not.toContain("user:password");
    expect(redacted).toContain("apiKey=[REDACTED]");
    expect(redacted).toContain('api_key: "[REDACTED]"');

    const escapedMultiline = redactFrontendLogText(
      '{"detail":"line1\\nsk-ant-api03-escaped-secret"}',
    );
    expect(escapedMultiline).not.toContain("sk-ant-api03-escaped-secret");
  });

  it("writes bounded, redacted errors through the Tauri log plugin", () => {
    const secret = "sensitive-token";
    const oversizedDetails = `${"x".repeat(2_000_000)} token=${secret}`;

    reportFrontendError(
      "window.error",
      new Error(`failed at https://example.test/?key=${secret}`),
      oversizedDetails,
    );

    expect(writeErrorLog).toHaveBeenCalledOnce();
    const [message, options] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain(secret);
    expect(message).toContain("[truncated]");
    expect(message.length).toBeLessThanOrEqual(12_020);
    expect(options).toEqual({ file: "frontend" });
  });

  it("serializes object-shaped rejection reasons without exposing secrets", () => {
    // 两层脱敏契约：
    //  - 属性名一级：命中敏感名(含复数/数组/嵌套)整个值一律隐藏；
    //  - 值一级：任意位置的“不透明密钥形状”按形状隐藏；
    //  - 文本一级：序列化后 `"name":"value"` 的裸密钥由正则兜底。
    reportFrontendError("unhandledrejection", {
      code: 500,
      message: "auth failed", // 良性文本保留(值含 "auth" 但非 name:value)
      key: "short-secret", // 裸 key 标量(非不透明) → 属性名层
      token: "object-secret", // 标量命名 → 属性名层
      tokens: ["k-9f3a7c2b1e"], // 复数+数组(短值) → 属性名层(去尾 s + 整体隐藏)
      auth: ["opaque-credential"], // 数组 → 属性名层
      credential: "AIzaRealCredential123",
      nested: { detail: "ghp_abcdef123456" }, // 非敏感名，靠不透明形状
      values: ["eyJhbGciOiJIUzI1NiJ9.cGF5bG9hZA.c2lnbmF0dXJl"], // 数组内不透明形状
      multiline: "line1\nsk-ant-api03-multiline-secret", // 串内不透明形状
      session: { activeTab: "providers", scrollPos: 120 }, // 良性状态原样保留
    });

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).toContain('"code":500');
    expect(message).toContain('"message":"auth failed"');
    expect(message).not.toContain("short-secret");
    expect(message).not.toContain("object-secret");
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).not.toContain("opaque-credential");
    expect(message).not.toContain("AIzaRealCredential123");
    expect(message).not.toContain("ghp_abcdef123456");
    expect(message).not.toContain("eyJhbGciOiJIUzI1NiJ9");
    expect(message).not.toContain("sk-ant-api03-multiline-secret");
    expect(message).toContain('"key":"[REDACTED]"');
    expect(message).toContain('"token":"[REDACTED]"');
    expect(message).toContain('"tokens":"[REDACTED]"');
    expect(message).toContain('"auth":"[REDACTED]"');
    expect(message).toContain('"credential":"[REDACTED]"');
    expect(message).toContain(
      '"session":{"activeTab":"providers","scrollPos":120}',
    );
  });

  it("applies property-level redaction to stringified-JSON rejection reasons", () => {
    // 字符串形态的 JSON 不能只靠文本正则——数组/复数/裸标量字段会漏。
    reportFrontendError(
      "unhandledrejection",
      '{"tokens":["k-9f3a7c2b1e"],"auth":["opaque-credential"],"key":"short-secret","keepMe":"visible"}',
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).not.toContain("opaque-credential");
    expect(message).not.toContain("short-secret");
    expect(message).toContain('"tokens":"[REDACTED]"');
    expect(message).toContain('"auth":"[REDACTED]"');
    expect(message).toContain('"key":"[REDACTED]"');
    expect(message).toContain('"keepMe":"visible"');
  });

  it("keeps non-JSON error strings as readable text", () => {
    reportFrontendError("window.error", "plain failure at step 3");

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).toContain("plain failure at step 3");
  });

  it("applies property-level redaction to JSON wrapped in an Error message", () => {
    // throw new Error(JSON.stringify(payload)) 会把凭据藏进 message，
    // 而 error.stack 第一行原样吐出 message —— 必须先对 message 结构化脱敏。
    reportFrontendError(
      "unhandledrejection",
      new Error(
        '{"tokens":["k-9f3a7c2b1e"],"key":"short-secret","keepMe":"visible"}',
      ),
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).not.toContain("short-secret");
    expect(message).toContain('"tokens":"[REDACTED]"');
    expect(message).toContain('"key":"[REDACTED]"');
    expect(message).toContain('"keepMe":"visible"');
  });

  it("omits oversized JSON error strings instead of leaking truncated fields", () => {
    // 合法但超长的 JSON 若先截断再 parse，必成非法 JSON 而退回文本层，数组字段泄漏。
    const padding = "x".repeat(20_000);
    reportFrontendError(
      "unhandledrejection",
      `{"tokens":["k-9f3a7c2b1e"],"padding":"${padding}"}`,
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).toContain("[oversized structured error omitted]");
  });

  it("omits oversized JSON wrapped in an Error message", () => {
    const padding = "x".repeat(20_000);
    reportFrontendError(
      "unhandledrejection",
      new Error(`{"tokens":["k-9f3a7c2b1e"],"padding":"${padding}"}`),
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).toContain("[oversized structured error omitted]");
  });

  it("redacts array credentials in prefix+JSON rejection strings", () => {
    // 前缀 + JSON：`redactStructuredString` 的 startsWith 门被前缀挡掉，退回文本层。
    // 文本层的容器正则必须在这个统一出口兜住 `"tokens":[...]`。
    reportFrontendError(
      "unhandledrejection",
      'Load failed: {"tokens":["ak_live_7f3d9b21c8e4"]}',
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("ak_live_7f3d9b21c8e4");
    expect(message).toContain("Load failed");
  });

  it("redacts array credentials in prefix+JSON wrapped in an Error", () => {
    reportFrontendError(
      "unhandledrejection",
      new Error(
        "Provider provisioning failed: " +
          JSON.stringify({ apiKeys: ["ak_live_7f3d9b21c8e4"] }),
      ),
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("ak_live_7f3d9b21c8e4");
  });

  it("redacts credentials in double-encoded nested JSON (escaped quotes)", () => {
    reportFrontendError(
      "unhandledrejection",
      new Error(
        JSON.stringify({
          status: 400,
          body: '{"keys":["abcd1234efgh5678ij"]}',
        }),
      ),
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("abcd1234efgh5678ij");
  });

  it("redacts array credentials in a POJO (non-Error) rejection", () => {
    reportFrontendError("unhandledrejection", {
      name: "HttpError",
      message: '{"tokens":["opaqueTokenValue12345"]}',
      code: 400,
    });

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("opaqueTokenValue12345");
  });

  it("redacts container values only under sensitive keys", () => {
    // 容器正则只对敏感键生效：普通数组/对象(items/config)与后缀含 key 的词(monkey)不误伤。
    const redacted = redactFrontendLogText(
      '{"tokens":["k-secret-1"],"monkey":["visible-a"],"items":["visible-b"],"config":{"theme":"dark"}}',
    );

    expect(redacted).not.toContain("k-secret-1");
    expect(redacted).toContain("visible-a");
    expect(redacted).toContain("visible-b");
    expect(redacted).toContain('"theme":"dark"');
  });

  it("preserves native WebKit-style stack frames for JSON-wrapped errors", () => {
    // macOS/Linux 的 WKWebView 用 `fn@file:line:col` 格式，且 stack 不含 message。
    // 旧的 `/^\s+at\s/` 过滤会把整段栈丢掉；新实现须补脱敏头并保留原生栈。
    const err = new Error('{"tokens":["k-9f3a7c2b1e"]}');
    Object.defineProperty(err, "stack", {
      value:
        "handleClick@tauri://localhost/assets/index.js:42:15\n" +
        "dispatch@tauri://localhost/assets/index.js:99:3",
    });

    reportFrontendError("unhandledrejection", err);

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).toContain('"tokens":"[REDACTED]"'); // 脱敏 message 头
    expect(message).toContain("handleClick@tauri://localhost"); // 原生栈帧保留
    expect(message).toContain("dispatch@tauri://localhost");
  });

  it("replaces every occurrence of the raw message in a V8-style stack", () => {
    // message 若在 stack 里出现多次(eval/匿名帧回显)，字面量替换必须全部换掉，零残留。
    const err = new Error('{"tokens":["k-9f3a7c2b1e"]}');
    Object.defineProperty(err, "stack", {
      value:
        'Error: {"tokens":["k-9f3a7c2b1e"]}\n' +
        '    at eval (eval at <anonymous>, {"tokens":["k-9f3a7c2b1e"]}:1:1)\n' +
        "    at run (index.js:10:5)",
    });

    reportFrontendError("unhandledrejection", err);

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("k-9f3a7c2b1e");
    expect(message).toContain("at run (index.js:10:5)"); // 栈帧保留
  });

  it("redacts standalone secret shapes in ordinary error text", () => {
    reportFrontendError(
      "window.error",
      new Error("request failed with sk-ant-api03-real-secret"),
    );

    const [message] = writeErrorLog.mock.calls[0];
    expect(message).not.toContain("sk-ant-api03-real-secret");
    expect(message).toContain("[REDACTED]");
  });

  it("handles circular rejection objects", () => {
    const reason: Record<string, unknown> = { message: "failed" };
    reason.self = reason;

    expect(() =>
      reportFrontendError("unhandledrejection", reason),
    ).not.toThrow();
    expect(writeErrorLog.mock.calls[0][0]).toContain("[Circular]");
  });

  it("captures global errors and unhandled rejections and can uninstall", () => {
    const target = new EventTarget() as unknown as Window;
    const uninstall = installGlobalErrorHandlers(target);

    const errorEvent = new Event("error") as ErrorEvent;
    Object.defineProperties(errorEvent, {
      error: { value: new Error("render failed") },
      filename: { value: "app.js" },
      lineno: { value: 10 },
      colno: { value: 4 },
    });
    target.dispatchEvent(errorEvent);

    const rejectionEvent = new Event(
      "unhandledrejection",
    ) as PromiseRejectionEvent;
    Object.defineProperty(rejectionEvent, "reason", {
      value: new Error("request failed"),
    });
    target.dispatchEvent(rejectionEvent);

    expect(writeErrorLog).toHaveBeenCalledTimes(2);

    uninstall();
    target.dispatchEvent(errorEvent);
    expect(writeErrorLog).toHaveBeenCalledTimes(2);
  });
});
