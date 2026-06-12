const { spawnSync } = require("node:child_process");
const path = require("node:path");

const repoRoot = path.resolve(__dirname, "..");
const tauriDir = path.join(repoRoot, "src-tauri");

const tests = [
  "responses_request_to_chat_does_not_emit_unanswered_tool_calls",
  "responses_request_to_chat_keeps_multiple_tool_calls_adjacent_to_outputs",
  "responses_request_to_chat_keeps_trailing_unanswered_tool_call_after_unmatched_tool_output",
  "test_anthropic_to_openai_drops_unanswered_parallel_tool_calls",
  "test_anthropic_to_openai_drops_orphan_tool_result",
  "test_anthropic_to_openai_keeps_trailing_pending_tool_use",
  "test_anthropic_to_openai_drops_tool_use_from_non_assistant_message",
  "codex_chat_history::tests",
  "transform_gemini::tests::anthropic_to_gemini_rejects_tool_result_without_resolvable_name",
  "streaming_gemini::tests::parallel_empty_string_id_calls_are_treated_as_missing_and_preserved",
  "streaming_responses::tests::test_streaming_conversion_interleaved_tool_deltas_by_item_id",
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
