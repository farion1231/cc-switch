import {
  chmodSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const sourceDir = resolve(
  process.env.CURSOR_BYOK_DIR ?? join(repoRoot, "sidecars", "cursor-byok"),
);
const outputDir = resolve(repoRoot, "src-tauri", "binaries");
const args = process.argv.slice(2);
const targetArg = args.find((argument) => !argument.startsWith("--"));
const testMode = args.includes("--test");
let target = targetArg ?? process.env.TAURI_ENV_TARGET_TRIPLE ?? "host";
const baseName = "cursor-sidecar";
const goproxyVersion = "v1.7.2";
const goproxyReplacementSource = join(
  sourceDir,
  "build",
  "overlays",
  "goproxy",
  "certs.go",
);
const patchedGoproxyDir = join(outputDir, ".goproxy-no-default-ca");
const temporaryGoMod = join(outputDir, "cursor-sidecar-build.mod");

const targets = new Map([
  [
    "aarch64-apple-darwin",
    { goos: "darwin", goarch: "arm64", format: "macho", machine: 0x0100000c },
  ],
  [
    "x86_64-apple-darwin",
    { goos: "darwin", goarch: "amd64", format: "macho", machine: 0x01000007 },
  ],
  [
    "aarch64-pc-windows-msvc",
    { goos: "windows", goarch: "arm64", format: "pe", machine: 0xaa64 },
  ],
  [
    "x86_64-pc-windows-msvc",
    { goos: "windows", goarch: "amd64", format: "pe", machine: 0x8664 },
  ],
  [
    "aarch64-unknown-linux-gnu",
    { goos: "linux", goarch: "arm64", format: "elf", machine: 183 },
  ],
  [
    "x86_64-unknown-linux-gnu",
    { goos: "linux", goarch: "amd64", format: "elf", machine: 62 },
  ],
]);

function fail(message) {
  console.error(message);
  process.exit(1);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: options.env ?? process.env,
    stdio: options.capture ? "pipe" : "inherit",
    encoding: options.capture ? "utf8" : undefined,
    shell: false,
  });
  if (result.error) fail(`${command} 启动失败: ${result.error.message}`);
  if (result.status !== 0) {
    if (options.capture) {
      process.stderr.write(result.stderr ?? "");
      process.stdout.write(result.stdout ?? "");
    }
    fail(`${command} 执行失败，退出码 ${result.status ?? "unknown"}`);
  }
  return result.stdout?.trim();
}

function generateProto() {
  const goPath =
    process.env.GOPATH ?? run("go", ["env", "GOPATH"], { capture: true });
  if (!goPath) fail("无法解析 GOPATH");
  const pluginDir = join(goPath, "bin");
  run(
    "protoc",
    [
      "-I",
      "./proto",
      "--go_out=.",
      "--go_opt=module=cursor",
      "--connect-go_out=.",
      "--connect-go_opt=module=cursor",
      "./proto/agent_v1.proto",
      "./proto/aiserver_v1.proto",
    ],
    {
      cwd: sourceDir,
      env: {
        ...process.env,
        PATH: `${pluginDir}${process.platform === "win32" ? ";" : ":"}${process.env.PATH ?? ""}`,
      },
    },
  );
}

function makeTreeWritable(path) {
  if (!existsSync(path)) return;
  const stat = lstatSync(path);
  if (stat.isSymbolicLink()) return;
  if (stat.isDirectory()) {
    chmodSync(path, stat.mode | 0o700);
    for (const entry of readdirSync(path)) {
      makeTreeWritable(join(path, entry));
    }
    return;
  }
  chmodSync(path, stat.mode | 0o600);
}

function preparePatchedDependencies() {
  const moduleJson = run(
    "go",
    [
      "mod",
      "download",
      "-json",
      `github.com/elazarl/goproxy@${goproxyVersion}`,
    ],
    { cwd: sourceDir, capture: true },
  );
  const upstreamDir = JSON.parse(moduleJson ?? "{}").Dir;
  if (!upstreamDir || !existsSync(goproxyReplacementSource)) {
    fail("无法准备不含默认 CA 的 goproxy 构建副本");
  }

  makeTreeWritable(patchedGoproxyDir);
  rmSync(patchedGoproxyDir, { recursive: true, force: true });
  cpSync(upstreamDir, patchedGoproxyDir, { recursive: true });
  makeTreeWritable(patchedGoproxyDir);
  const patchedGoproxyCerts = join(patchedGoproxyDir, "certs.go");
  cpSync(goproxyReplacementSource, patchedGoproxyCerts);

  const sourceGoMod = readFileSync(join(sourceDir, "go.mod"), "utf8");
  const patchedModulePath = patchedGoproxyDir.replaceAll("\\", "/");
  writeFileSync(
    temporaryGoMod,
    `${sourceGoMod}\nreplace github.com/elazarl/goproxy => ${JSON.stringify(patchedModulePath)}\n`,
  );
  cpSync(join(sourceDir, "go.sum"), `${temporaryGoMod.slice(0, -4)}.sum`);
  return temporaryGoMod;
}

function findEmbeddedPrivateKey(binaryPath) {
  const binaryText = readFileSync(binaryPath).toString("latin1");
  const privateKeyBlock =
    /-----BEGIN ((?:RSA |EC )?PRIVATE KEY)-----[\r\n]+((?:[A-Za-z0-9+/]{64}[\r\n]+)+(?:[A-Za-z0-9+/]{1,63}={0,2}[\r\n]+)?)-----END \1-----/g;
  return [...binaryText.matchAll(privateKeyBlock)].find((match) => {
    const der = Buffer.from(match[2].replace(/\s/g, ""), "base64");
    return der.length > 0 && der[0] === 0x30;
  });
}

function verifyNoEmbeddedPrivateKey(binaryPath) {
  if (findEmbeddedPrivateKey(binaryPath)) {
    fail(`sidecar 包含嵌入的有效 PEM 私钥: ${binaryPath}`);
  }
}

function verifyBinary(targetTriple, binaryPath) {
  const platform = targets.get(targetTriple);
  if (!platform) fail(`无法验证未知 target: ${targetTriple}`);
  verifyNoEmbeddedPrivateKey(binaryPath);
  const bytes = readFileSync(binaryPath);

  if (platform.format === "macho") {
    const magic = bytes.readUInt32LE(0);
    const machine = bytes.readUInt32LE(4);
    if (magic !== 0xfeedfacf || machine !== platform.machine) {
      fail(`sidecar Mach-O 架构不匹配: ${targetTriple}`);
    }
  } else if (platform.format === "pe") {
    if (bytes.toString("ascii", 0, 2) !== "MZ") {
      fail(`sidecar 不是有效 PE: ${targetTriple}`);
    }
    const peOffset = bytes.readUInt32LE(0x3c);
    const signature = bytes.toString("ascii", peOffset, peOffset + 4);
    const machine = bytes.readUInt16LE(peOffset + 4);
    if (signature !== "PE\0\0" || machine !== platform.machine) {
      fail(`sidecar PE 架构不匹配: ${targetTriple}`);
    }
  } else {
    const isElf =
      bytes[0] === 0x7f &&
      bytes[1] === 0x45 &&
      bytes[2] === 0x4c &&
      bytes[3] === 0x46;
    const machine = bytes.readUInt16LE(18);
    if (!isElf || machine !== platform.machine) {
      fail(`sidecar ELF 架构不匹配: ${targetTriple}`);
    }
  }
}

function testSidecarDependencies(goModPath) {
  const dependencies = run(
    "go",
    [
      "list",
      "-modfile",
      goModPath,
      "-deps",
      "-f",
      "{{.ImportPath}}",
      "./cmd/cc-switch-sidecar",
    ],
    {
      cwd: sourceDir,
      env: { ...process.env, CGO_ENABLED: "0" },
      capture: true,
    },
  )?.split(/\r?\n/);
  if (dependencies?.some((name) => name.startsWith("github.com/wailsapp/"))) {
    fail("Cursor sidecar 依赖图包含 Wails");
  }
  const packages = dependencies?.filter((name) => name.startsWith("cursor/"));
  if (!packages?.length) fail("未解析到 Cursor sidecar 的模块内依赖");
  run("go", ["test", "-modfile", goModPath, ...packages], {
    cwd: sourceDir,
    env: { ...process.env, CGO_ENABLED: "0" },
  });
}

function outputPath(targetTriple) {
  const extension = targetTriple.includes("windows") ? ".exe" : "";
  return join(outputDir, `${baseName}-${targetTriple}${extension}`);
}

function build(targetTriple, destination, goModPath) {
  const platform = targets.get(targetTriple);
  if (!platform) fail(`不支持的 Cursor sidecar target: ${targetTriple}`);

  run(
    "go",
    [
      "build",
      "-modfile",
      goModPath,
      "-trimpath",
      "-ldflags=-s -w",
      "-o",
      destination,
      "./cmd/cc-switch-sidecar",
    ],
    {
      cwd: sourceDir,
      env: {
        ...process.env,
        CGO_ENABLED: "0",
        GOOS: platform.goos,
        GOARCH: platform.goarch,
      },
    },
  );
}

if (target === "host") {
  const rustcInfo = run("rustc", ["-vV"], { capture: true });
  target = rustcInfo
    ?.split(/\r?\n/)
    .find((line) => line.startsWith("host: "))
    ?.slice("host: ".length);
}
if (!target) {
  fail(
    "缺少 target triple。用法: node scripts/build-cursor-sidecar.mjs <target-triple|host>",
  );
}
if (!existsSync(join(sourceDir, "go.mod"))) {
  fail(
    [
      `Cursor sidecar 源码不存在或 submodule 未初始化: ${sourceDir}`,
      "请运行: git submodule update --init --recursive",
    ].join("\n"),
  );
}

generateProto();
mkdirSync(outputDir, { recursive: true });
const goModPath = preparePatchedDependencies();
if (testMode) testSidecarDependencies(goModPath);

if (target === "universal-apple-darwin") {
  if (process.platform !== "darwin") {
    fail("universal-apple-darwin 必须在 macOS 上使用 lipo 构建");
  }
  const arm64 = outputPath("aarch64-apple-darwin");
  const amd64 = outputPath("x86_64-apple-darwin");
  build("aarch64-apple-darwin", arm64, goModPath);
  build("x86_64-apple-darwin", amd64, goModPath);
  verifyBinary("aarch64-apple-darwin", arm64);
  verifyBinary("x86_64-apple-darwin", amd64);
  run("lipo", ["-create", "-output", outputPath(target), arm64, amd64]);
  run("lipo", [outputPath(target), "-verify_arch", "arm64", "x86_64"]);
} else {
  build(target, outputPath(target), goModPath);
  verifyBinary(target, outputPath(target));
}

console.log(`Cursor sidecar ready: ${outputPath(target)}`);
