import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../", import.meta.url));
const argument = (name) => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
};
const code =
  argument("--code") ||
  process.env.VSCODE_EXECUTABLE ||
  (process.platform === "win32"
    ? path.join(process.env.LOCALAPPDATA, "Programs/Microsoft VS Code/Code.exe")
    : process.platform === "darwin"
      ? "/Applications/Visual Studio Code.app/Contents/MacOS/Electron"
      : "/usr/share/code/code");
const root = await fs.mkdtemp(path.join(os.tmpdir(), "cc-switch-vscode-e2e-"));
console.log("VS Code smoke artifacts: " + root);

async function findCopilotExtension() {
  const configured = argument("--copilot-extension");
  if (configured) return path.resolve(configured);
  const directory = path.dirname(code);
  const candidates = [
    path.join(directory, "resources/app/extensions/copilot"),
    path.resolve(directory, "../Resources/app/extensions/copilot"),
    path.resolve(directory, "../resources/app/extensions/copilot"),
  ];
  for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory())
      candidates.push(
        path.join(directory, entry.name, "resources/app/extensions/copilot"),
      );
  }
  for (const candidate of candidates) {
    try {
      const metadata = JSON.parse(
        await fs.readFile(path.join(candidate, "package.json"), "utf8"),
      );
      if (metadata.publisher === "GitHub" && metadata.name === "copilot-chat")
        return candidate;
    } catch {
      /* Try the next installed version. */
    }
  }
  throw new Error(
    "Provide --copilot-extension with the installed Copilot extension directory",
  );
}

async function runProcess(executable, args, env, prefix, timeoutMs) {
  const output = createWriteStream(path.join(root, prefix + ".stdout.log"));
  const errors = createWriteStream(path.join(root, prefix + ".stderr.log"));
  let text = "";
  const child = spawn(executable, args, { cwd: repo, env, windowsHide: true });
  child.stdout.pipe(output);
  child.stderr.pipe(errors);
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    text += chunk;
  });
  return await new Promise((resolve, reject) => {
    let settled = false;
    const finish = (error, exitCode) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      output.end();
      errors.end();
      if (error) reject(error);
      else if (exitCode !== 0)
        reject(new Error(prefix + " exited " + exitCode + ". See artifacts."));
      else resolve(text);
    };
    const timer = setTimeout(() => {
      child.kill();
      child.stdout.destroy();
      child.stderr.destroy();
      finish(new Error(prefix + " timed out"), null);
    }, timeoutMs);
    child.once("error", (error) => finish(error, null));
    child.once("close", (exitCode) => finish(null, exitCode));
  });
}

function sendEvent(response, type, value) {
  response.write(
    "event: " + type + "\ndata: " + JSON.stringify({ type, ...value }) + "\n\n",
  );
}

function sendReply(response, testCase) {
  response.writeHead(200, {
    "Content-Type": "text/event-stream",
    "Cache-Control": "no-cache",
  });
  const text = testCase.expected;
  const model = testCase.wire;
  if (testCase.apiType === "messages") {
    sendEvent(response, "message_start", {
      message: {
        id: "msg_smoke",
        type: "message",
        role: "assistant",
        model,
        content: [],
        stop_reason: null,
        usage: { input_tokens: 10, output_tokens: 0 },
      },
    });
    sendEvent(response, "content_block_start", {
      index: 0,
      content_block: { type: "text", text: "" },
    });
    sendEvent(response, "content_block_delta", {
      index: 0,
      delta: { type: "text_delta", text },
    });
    sendEvent(response, "content_block_stop", { index: 0 });
    sendEvent(response, "message_delta", {
      delta: { stop_reason: "end_turn", stop_sequence: null },
      usage: { output_tokens: 5 },
    });
    sendEvent(response, "message_stop", {});
  } else if (testCase.apiType === "responses") {
    const item = {
      id: "msg_smoke",
      type: "message",
      role: "assistant",
      status: "completed",
      content: [{ type: "output_text", text, annotations: [] }],
    };
    const base = { id: "resp_smoke", object: "response", model, created_at: 1 };
    sendEvent(response, "response.created", {
      sequence_number: 0,
      response: { ...base, status: "in_progress", output: [] },
    });
    sendEvent(response, "response.output_item.added", {
      sequence_number: 1,
      output_index: 0,
      item: { ...item, status: "in_progress", content: [] },
    });
    sendEvent(response, "response.content_part.added", {
      sequence_number: 2,
      item_id: item.id,
      output_index: 0,
      content_index: 0,
      part: { type: "output_text", text: "", annotations: [] },
    });
    sendEvent(response, "response.output_text.delta", {
      sequence_number: 3,
      item_id: item.id,
      output_index: 0,
      content_index: 0,
      delta: text,
    });
    sendEvent(response, "response.output_text.done", {
      sequence_number: 4,
      item_id: item.id,
      output_index: 0,
      content_index: 0,
      text,
    });
    sendEvent(response, "response.content_part.done", {
      sequence_number: 5,
      item_id: item.id,
      output_index: 0,
      content_index: 0,
      part: item.content[0],
    });
    sendEvent(response, "response.output_item.done", {
      sequence_number: 6,
      output_index: 0,
      item,
    });
    sendEvent(response, "response.completed", {
      sequence_number: 7,
      response: {
        ...base,
        status: "completed",
        output: [item],
        usage: {
          input_tokens: 10,
          output_tokens: 5,
          total_tokens: 15,
          input_tokens_details: { cached_tokens: 0 },
        },
      },
    });
  } else {
    const chunk = (delta, finish) => ({
      id: "chatcmpl-smoke",
      object: "chat.completion.chunk",
      created: 1,
      model,
      choices: [{ index: 0, delta, finish_reason: finish }],
    });
    response.write(
      "data: " +
        JSON.stringify(chunk({ role: "assistant", content: text }, null)) +
        "\n\n",
    );
    response.write(
      "data: " + JSON.stringify(chunk({}, "stop")) + "\n\ndata: [DONE]\n\n",
    );
  }
  response.end();
}

let fixture;
const requests = [];
const server = http.createServer(async (request, response) => {
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  try {
    const body = JSON.parse(Buffer.concat(chunks).toString() || "{}");
    const testCase = fixture?.cases.find((entry) => entry.path === request.url);
    assert.ok(testCase, "Unexpected endpoint");
    assert.equal(body.model, testCase.wire, "Wrong request model");
    const expectedHeaders = {
      [testCase.authHeader]: testCase.authValue,
      ...testCase.additionalHeaders,
    };
    for (const [header, value] of Object.entries(expectedHeaders)) {
      assert.ok(
        request.headers[header] === value,
        "Authentication header mismatch",
      );
    }
    for (const header of ["authorization", "api-key", "x-api-key"]) {
      if (!(header in expectedHeaders)) {
        assert.ok(
          request.headers[header] === undefined,
          "Conflicting authentication header",
        );
      }
    }
    requests.push({
      name: testCase.name,
      path: request.url,
      model: body.model,
      authenticationMatches: true,
    });
    sendReply(response, testCase);
  } catch (error) {
    requests.push({ path: request.url, error: error.message });
    response.writeHead(400, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ error: { message: error.message } }));
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));

try {
  await fs.access(code);
  const copilot = await findCopilotExtension();
  const env = {
    ...process.env,
    CC_SWITCH_VSCODE_SMOKE_ORIGIN: "http://127.0.0.1:" + server.address().port,
  };
  delete env.ELECTRON_RUN_AS_NODE;
  delete env.VSCODE_DEV;
  for (const directory of [
    "home/AppData/Roaming",
    "home/AppData/Local",
    "temp",
    "extensions",
    "workspace",
    "shared",
  ]) {
    await fs.mkdir(path.join(root, directory), { recursive: true });
  }
  Object.assign(env, {
    CC_SWITCH_TEST_HOME: path.join(root, "home"),
    APPDATA: path.join(root, "home/AppData/Roaming"),
    LOCALAPPDATA: path.join(root, "home/AppData/Local"),
    TEMP: path.join(root, "temp"),
    TMP: path.join(root, "temp"),
  });
  const output = await runProcess(
    "cargo",
    [
      "test",
      "--locked",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--lib",
      "copilot_vscode",
      "--",
      "--include-ignored",
      "--nocapture",
    ],
    env,
    "fixture",
    15 * 60 * 1000,
  );
  const match = output.match(/CC_SWITCH_VSCODE_FIXTURE=(.+)/);
  assert.ok(match, "Rust did not export the fixture");
  fixture = JSON.parse(match[1]);
  const fixturePath = path.join(root, "fixture.json");
  const reportPath = path.join(root, "result.json");
  await fs.writeFile(fixturePath, JSON.stringify(fixture, null, 2));
  await fs.writeFile(
    path.join(fixture.userDataDir, "User/settings.json"),
    JSON.stringify(
      {
        "telemetry.telemetryLevel": "off",
        "update.mode": "none",
        "extensions.autoUpdate": false,
        "security.workspace.trust.enabled": false,
        "workbench.startupEditor": "none",
        "chat.disableAIFeatures": false,
      },
      null,
      2,
    ),
  );
  Object.assign(env, {
    CC_SWITCH_VSCODE_SMOKE_FIXTURE: fixturePath,
    CC_SWITCH_VSCODE_SMOKE_REPORT: reportPath,
  });
  await runProcess(
    code,
    [
      "--user-data-dir",
      fixture.userDataDir,
      "--shared-data-dir",
      path.join(root, "shared"),
      "--extensions-dir",
      path.join(root, "extensions"),
      "--extensionDevelopmentPath",
      path.join(repo, "tests/vscode/copilot-sync"),
      "--extensionDevelopmentPath",
      copilot,
      "--enable-proposed-api",
      "GitHub.copilot-chat",
      "--disable-workspace-trust",
      "--skip-welcome",
      "--skip-release-notes",
      "--wait",
      path.join(root, "workspace"),
    ],
    env,
    "vscode",
    180000,
  );
  const report = JSON.parse(await fs.readFile(reportPath, "utf8"));
  report.requests = requests;
  await fs.writeFile(reportPath, JSON.stringify(report, null, 2));
  assert.ok(report.success, report.error || "VS Code test failed");
  for (const testCase of fixture.cases) {
    assert.ok(
      requests.some(
        (request) =>
          request.name === testCase.name && request.authenticationMatches,
      ),
      "Request missing: " + testCase.name,
    );
  }
  console.log(JSON.stringify(report, null, 2));
} finally {
  server.closeAllConnections();
  server.close();
}
