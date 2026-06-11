const { spawnSync } = require("node:child_process");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..");
const tauriDir = path.join(repoRoot, "src-tauri");

const tests = [
  "responses_request_to_chat_does_not_emit_unanswered_tool_calls",
  "responses_request_to_chat_keeps_multiple_tool_calls_adjacent_to_outputs",
  "codex_chat_history::tests",
];

for (const testName of tests) {
  const result = spawnSync("cargo", ["test", testName], {
    cwd: tauriDir,
    stdio: "inherit",
    shell: process.platform === "win32",
  });

  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
