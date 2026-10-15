import assert from "node:assert/strict";
import { mkdtemp, cp, rm } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import test from "node:test";

const repositoryRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const sourceTemplate = path.join(
  repositoryRoot,
  "src-tauri",
  "resources",
  "api-key-var-template",
);
const temporaryRoot = await mkdtemp(
  path.join(os.tmpdir(), "cc-switch API Key 模板 "),
);
assert.equal(path.dirname(temporaryRoot), path.resolve(os.tmpdir()));
const templateDirectory = path.join(temporaryRoot, "中文 空格 模板");
await cp(sourceTemplate, templateDirectory, { recursive: true });
const safeTemplateEnvironment = {
  // Do not let a developer's real provider configuration influence this harness.
  API_KEY_TEMPLATE_MODE: "demo",
  API_KEY_REFRESH_URL: "https://api.example.invalid/v1/refresh-key",
  API_KEY_REFRESH_TIMEOUT_SECONDS: "20",
  API_KEY_REFRESH_TOKEN: undefined,
};
const offlineEnvironment = {
  API_KEY_TEMPLATE_MODE: "demo",
  API_KEY_REFRESH_TIMEOUT_SECONDS: "20",
};

function runTemplate(environment = {}) {
  return new Promise((resolve, reject) => {
    const childEnvironment = {
      ...process.env,
      ...safeTemplateEnvironment,
      ...environment,
    };
    for (const [name, value] of Object.entries(childEnvironment)) {
      if (value === undefined) delete childEnvironment[name];
    }
    const child = spawn(process.execPath, ["index.mjs"], {
      cwd: templateDirectory,
      env: childEnvironment,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

let route = "success";
let receivedAuthorization;
const server = http.createServer((request, response) => {
  receivedAuthorization = request.headers.authorization;
  if (route === "timeout") return; // client-side AbortController must handle this.
  if (route === "truncated") {
    response.writeHead(200, { "content-type": "application/json" });
    response.write('{"apiKey":"partial');
    return response.destroy();
  }
  if (route === "auth") {
    response.writeHead(401, { "content-type": "application/json" });
    return response.end(JSON.stringify({ detail: "credential rejected" }));
  }
  if (route === "forbidden") {
    response.writeHead(403, { "content-type": "application/json" });
    return response.end(JSON.stringify({ detail: "forbidden" }));
  }
  if (route === "408" || route === "429") {
    response.writeHead(Number(route), { "content-type": "application/json" });
    return response.end(JSON.stringify({ detail: "retry later" }));
  }
  if (route === "redirect") {
    response.writeHead(302, { location: "https://example.invalid/other" });
    return response.end();
  }
  if (route === "retry") {
    response.writeHead(503, { "content-type": "application/json" });
    return response.end(JSON.stringify({ detail: "temporary failure" }));
  }
  if (route === "invalid") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "bad key with spaces",
        secret: "raw-provider-secret",
      }),
    );
  }
  if (route === "nel-key") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ apiKey: "local\u0085http-key" }));
  }
  if (route === "lone-high-key") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({ apiKey: `local${String.fromCharCode(0xd800)}key` }),
    );
  }
  if (route === "lone-low-message") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        message: `message${String.fromCharCode(0xdc00)}`,
      }),
    );
  }
  if (route === "giant-key") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ apiKey: "x".repeat(70 * 1024) }));
  }
  if (route === "null") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end("null");
  }
  if (route === "array") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end("[]");
  }
  if (route === "invalid-json") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end("not json");
  }
  if (route === "malformed-time") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: "2030-01-01 00:00:00",
      }),
    );
  }
  if (route === "invalid-calendar") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: "2030-02-29T00:00:00Z",
      }),
    );
  }
  if (route === "invalid-clock") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: "2032-02-29T24:00:00Z",
      }),
    );
  }
  if (route === "invalid-offset") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: "2032-02-29T00:00:00+24:60",
      }),
    );
  }
  if (route === "expired") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: "2000-01-01T00:00:00Z",
      }),
    );
  }
  if (route === "milliseconds") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        expiresAt: Date.now() + 3_600_000,
      }),
    );
  }
  if (route === "optional") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(JSON.stringify({ apiKey: "local-http-key" }));
  }
  if (route === "emoji-message") {
    response.writeHead(200, { "content-type": "application/json" });
    return response.end(
      JSON.stringify({
        apiKey: "local-http-key",
        message: `${"a".repeat(499)}😀tail`,
      }),
    );
  }
  response.writeHead(200, { "content-type": "application/json" });
  response.end(
    JSON.stringify({
      apiKey: "local-http-key",
      expiresAt: new Date(Date.now() + 3_600_000).toISOString(),
      refreshAfter: Math.floor(Date.now() / 1000) + 600,
      message: "local test",
    }),
  );
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();
const httpEnvironment = {
  API_KEY_TEMPLATE_MODE: "http",
  API_KEY_REFRESH_URL: `http://127.0.0.1:${port}/refresh`,
  API_KEY_REFRESH_TOKEN: "local-test-token",
};

test.after(async () => {
  await new Promise((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
  await rm(temporaryRoot, { recursive: true, force: true });
});

test("offline demo executes from a Chinese path and prints exactly one contract JSON", async () => {
  const result = await runTemplate(offlineEnvironment);
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.equal(result.stdout.split("\n").filter(Boolean).length, 1);
  const output = JSON.parse(result.stdout);
  assert.match(output.apiKey, /^demo-not-a-real-key-/);
  assert.ok(Date.parse(output.expiresAt) > Date.now());
});

test("HTTP success uses bearer auth and has contract-only stdout", async () => {
  route = "success";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.equal(receivedAuthorization, "Bearer local-test-token");
  assert.deepEqual(JSON.parse(result.stdout), {
    apiKey: "local-http-key",
    expiresAt: JSON.parse(result.stdout).expiresAt,
    refreshAfter: JSON.parse(result.stdout).refreshAfter,
    message: "local test",
  });
  const output = JSON.parse(result.stdout);
  assert.match(
    output.expiresAt,
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/,
  );
  assert.equal(typeof output.refreshAfter, "number");
});

test("an HTTP response may omit both optional scheduling fields", async () => {
  route = "optional";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), { apiKey: "local-http-key" });
});

test("message truncation preserves a complete emoji at the character boundary", async () => {
  route = "emoji-message";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 0, result.stderr);
  const output = JSON.parse(result.stdout);
  assert.equal(Array.from(output.message).length, 500);
  assert.ok(output.message.endsWith("😀"));
});

test("authentication failures are permanent and keep secrets out of stderr", async () => {
  route = "auth";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /rejected the refresh credential/);
  assert.doesNotMatch(result.stderr, /local-test-token|credential rejected/);
});

test("missing refresh credentials are permanent", async () => {
  route = "success";
  const result = await runTemplate({
    ...httpEnvironment,
    API_KEY_REFRESH_TOKEN: undefined,
  });
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /API_KEY_REFRESH_TOKEN is not set/);
});

test("invalid configuration is permanent before any request", async () => {
  const invalidMode = await runTemplate({ API_KEY_TEMPLATE_MODE: "wrong" });
  assert.equal(invalidMode.code, 2);
  assert.equal(invalidMode.stdout, "");
  const placeholder = await runTemplate({
    ...httpEnvironment,
    API_KEY_REFRESH_URL: "https://api.example.invalid/refresh",
  });
  assert.equal(placeholder.code, 2);
  assert.match(placeholder.stderr, /Replace the example URL/);
  const invalidTimeout = await runTemplate({
    ...httpEnvironment,
    API_KEY_REFRESH_TIMEOUT_SECONDS: "4",
  });
  assert.equal(invalidTimeout.code, 2);
});

test("HTTP 5xx is retryable", async () => {
  route = "retry";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 1);
  assert.match(result.stderr, /temporarily unavailable/);
});

test("HTTP 408 and 429 are retryable while 403 is permanent", async () => {
  for (const expectedRoute of ["408", "429"]) {
    route = expectedRoute;
    assert.equal((await runTemplate(httpEnvironment)).code, 1);
  }
  route = "forbidden";
  assert.equal((await runTemplate(httpEnvironment)).code, 2);
});

test("HTTP redirects are rejected permanently without following an auth-bearing POST", async () => {
  route = "redirect";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.doesNotMatch(result.stderr, /local-test-token/);
});

test("invalid provider responses are permanent and do not reveal raw data", async () => {
  route = "invalid";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.doesNotMatch(
    result.stderr,
    /bad key with spaces|raw-provider-secret|local-test-token/,
  );
});

test("Unicode NEL and lone surrogate values are rejected permanently", async () => {
  for (const expectedRoute of [
    "nel-key",
    "lone-high-key",
    "lone-low-message",
  ]) {
    route = expectedRoute;
    const result = await runTemplate(httpEnvironment);
    assert.equal(result.code, 2, `${expectedRoute}: ${result.stderr}`);
    assert.equal(result.stdout, "");
    assert.doesNotMatch(result.stderr, /local-test-token/);
  }
});

test("successful stdout over 64 KiB is rejected without printing the API key", async () => {
  route = "giant-key";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /64 KiB contract limit/);
  assert.doesNotMatch(result.stderr, /xxxxx|local-test-token/);
});

test("non-object, invalid JSON, invalid calendar/clock/offset, expired time and milliseconds are permanent", async () => {
  for (const expectedRoute of [
    "null",
    "array",
    "invalid-json",
    "malformed-time",
    "invalid-calendar",
    "invalid-clock",
    "invalid-offset",
    "expired",
    "milliseconds",
  ]) {
    route = expectedRoute;
    const result = await runTemplate(httpEnvironment);
    assert.equal(result.code, 2, `${expectedRoute}: ${result.stderr}`);
    assert.equal(result.stdout, "");
    assert.doesNotMatch(result.stderr, /local-test-token/);
  }
});

test("a refused local connection is retryable without leaking the token", async () => {
  const result = await runTemplate({
    ...httpEnvironment,
    API_KEY_REFRESH_URL: "http://127.0.0.1:1/refresh",
  });
  assert.equal(result.code, 1);
  assert.equal(result.stdout, "");
  assert.doesNotMatch(result.stderr, /local-test-token/);
});

test("headers-stall timeouts are retryable", async () => {
  route = "timeout";
  const result = await runTemplate({
    ...httpEnvironment,
    API_KEY_REFRESH_TIMEOUT_SECONDS: "5",
  });
  assert.equal(result.code, 1);
  assert.match(result.stderr, /timed out/);
});

test("truncated HTTP response bodies are retryable", async () => {
  route = "truncated";
  const result = await runTemplate(httpEnvironment);
  assert.equal(result.code, 1);
  assert.equal(result.stdout, "");
  assert.doesNotMatch(result.stderr, /local-test-token/);
});
