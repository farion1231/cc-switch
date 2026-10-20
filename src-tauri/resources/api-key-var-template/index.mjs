import { getConfiguration } from "./config.mjs";
import {
  PermanentError,
  RetryableError,
  validateRefreshResult,
} from "./lib/contract.mjs";
import { getDemoKey, getHttpKey } from "./lib/provider.mjs";

async function main() {
  const configuration = getConfiguration();
  const result =
    configuration.mode === "demo"
      ? await getDemoKey()
      : await getHttpKey(configuration.http);
  // stdout is reserved for exactly one successful contract JSON object.
  const stdout = `${JSON.stringify(validateRefreshResult(result))}\n`;
  if (Buffer.byteLength(stdout, "utf8") > 64 * 1024) {
    throw new PermanentError(
      "Script output exceeds the 64 KiB contract limit.",
    );
  }
  process.stdout.write(stdout);
}

main().catch((error) => {
  // Never print the token, an API key, request body, or provider response.
  const message =
    error instanceof PermanentError || error instanceof RetryableError
      ? error.message
      : "Unexpected script error.";
  process.stderr.write(`API key refresh failed: ${message}\n`);
  process.exitCode = error instanceof PermanentError ? 2 : 1;
});
