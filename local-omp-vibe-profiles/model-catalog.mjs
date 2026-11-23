import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execute = promisify(execFile);
const efforts = new Set([
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "auto",
]);

export function normalizeCatalog(payload) {
  if (!Array.isArray(payload?.models))
    throw new Error("OMP 返回了无法识别的模型目录。");
  const seen = new Map();
  for (const row of payload.models) {
    if (
      row?.kind !== "chat" ||
      typeof row.provider !== "string" ||
      typeof row.selector !== "string" ||
      !row.selector.startsWith(`${row.provider}/`)
    )
      continue;
    seen.set(row.selector, {
      provider: row.provider,
      selector: row.selector,
      name: typeof row.name === "string" && row.name ? row.name : row.selector,
      thinking: Array.isArray(row.thinking)
        ? [...new Set(row.thinking.filter((level) => efforts.has(level)))]
        : [],
    });
  }
  return [...seen.values()].sort(
    (a, b) =>
      a.provider.localeCompare(b.provider) || a.name.localeCompare(b.name),
  );
}

export function createModelCatalog(agentDir, explicit) {
  let pending;
  return async () => {
    pending ??= execute("omp", ["models", "--json", "--no-extensions"], {
      cwd: agentDir,
      env: explicit
        ? {
            ...process.env,
            OMP_PROFILE: "default",
            PI_PROFILE: "default",
            PI_CODING_AGENT_DIR: agentDir,
          }
        : process.env,
      encoding: "utf8",
      timeout: 30_000,
      maxBuffer: 8 * 1024 * 1024,
      windowsHide: true,
    })
      .then(({ stdout }) => normalizeCatalog(JSON.parse(stdout)))
      .catch(() => {
        throw new Error("无法读取 OMP 模型目录，请确认 OMP 已安装并重试。");
      })
      .finally(() => {
        pending = undefined;
      });
    return pending;
  };
}
