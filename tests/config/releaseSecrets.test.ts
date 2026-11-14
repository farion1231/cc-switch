import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";

const action = readFileSync(
  ".github/actions/prepare-tauri-signing-key/action.yml",
  "utf8",
);
const workflow = readFileSync(".github/workflows/release.yml", "utf8");
const directories: string[] = [];

afterEach(() => {
  for (const directory of directories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), "release-secrets-"));
  directories.push(directory);
  const envFile = join(directory, "github-env");
  writeFileSync(envFile, "");
  return {
    directory,
    envFile,
    marker: join(directory, "executed"),
    env: { ...process.env, RUNNER_TEMP: directory, GITHUB_ENV: envFile },
  };
}

function runBlock(source: string, name: string, stepIndent: string) {
  const marker = `${stepIndent}- name: ${name}\n`;
  const start = source.indexOf(marker);
  expect(start).toBeGreaterThanOrEqual(0);
  const sibling = `\n${stepIndent}- name:`;
  const next = source.indexOf(sibling, start + marker.length);
  const step = source.slice(start, next === -1 ? undefined : next);
  const keyIndent = `${stepIndent}  `;
  const runMarker = `${keyIndent}run: |\n`;
  const runStart = step.indexOf(runMarker);
  expect(runStart).toBeGreaterThanOrEqual(0);
  const bodyIndent = `${keyIndent}  `;
  return step
    .slice(runStart + runMarker.length)
    .split("\n")
    .map((line) =>
      line.startsWith(bodyIndent) ? line.slice(bodyIndent.length) : line,
    )
    .join("\n");
}

it("keeps shell syntax in the Tauri signing password literal", () => {
  const { directory, envFile, marker, env } = fixture();
  const password = `$(touch '${marker}')`;
  execFileSync(
    "bash",
    ["-c", runBlock(action, "Prepare Tauri signing key", "    ")],
    {
      cwd: directory,
      env: { ...env, RAW: "QUJD", KEY_PASSWORD: password },
    },
  );
  expect(existsSync(marker)).toBe(false);
  expect(readFileSync(envFile, "utf8")).toContain(
    `TAURI_SIGNING_PRIVATE_KEY_PASSWORD=${password}\n`,
  );
});

it("does not log a private key prefix when its format is invalid", () => {
  const { directory, env } = fixture();
  const privateKey = "SYNTHETIC_SECRET_INVALID!";
  const result = spawnSync(
    "bash",
    ["-c", runBlock(action, "Prepare Tauri signing key", "    ")],
    {
      cwd: directory,
      env: { ...env, RAW: privateKey, KEY_PASSWORD: "" },
      encoding: "utf8",
    },
  );
  expect(result.status).toBe(1);
  expect(`${result.stdout}${result.stderr}`).not.toContain(
    privateKey.slice(0, 10),
  );
});

it("passes Apple signing passwords as literal arguments to security", () => {
  const { directory, marker, env } = fixture();
  const bin = join(directory, "bin");
  const securityLog = join(directory, "security-arguments");
  mkdirSync(bin);
  const security = join(bin, "security");
  // Stub every security invocation: this test never accesses a real keychain.
  writeFileSync(
    security,
    '#!/bin/sh\nprintf "%s\\n" "$@" >> "$SECURITY_LOG"\ncase "$1" in\n  default-keychain) echo "fixture.keychain-db";;\n  find-identity) echo \'1) "Developer ID Application: Fixture"\';;\nesac\n',
  );
  chmodSync(security, 0o755);
  const certificatePassword = `$(touch '${marker}')`;
  const keychainPassword = `$(touch '${marker}')`;
  execFileSync(
    "bash",
    ["-c", runBlock(workflow, "Import Apple signing certificate", "      ")],
    {
      cwd: directory,
      env: {
        ...env,
        PATH: `${bin}:${process.env.PATH}`,
        SECURITY_LOG: securityLog,
        APPLE_CERTIFICATE: Buffer.from("synthetic certificate").toString(
          "base64",
        ),
        APPLE_CERTIFICATE_PASSWORD: certificatePassword,
        KEYCHAIN_PASSWORD: keychainPassword,
      },
    },
  );
  expect(existsSync(marker)).toBe(false);
  const argumentsSeen = readFileSync(securityLog, "utf8");
  expect(argumentsSeen).toContain(certificatePassword);
  expect(argumentsSeen).toContain(keychainPassword);
});
