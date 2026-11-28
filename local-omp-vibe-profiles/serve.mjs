import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import autoprefixer from "autoprefixer";
import tailwindcss from "tailwindcss";
import { createServer } from "vite";
import { createModelCatalog, normalizeCatalog } from "./model-catalog.mjs";
import { createStore } from "./vibe-config.mjs";

const args = process.argv.slice(2);
const live = args[0] === "--live" && args.length === 1;
const explicit = args[0] === "--agent-dir" && args.length === 2;
if (args.length && !live && !explicit) {
  console.error(
    "用法：node local-omp-vibe-profiles/serve.mjs [--live | --agent-dir <OMP agent 目录>]",
  );
  process.exit(1);
}
const root = path.dirname(fileURLToPath(import.meta.url));
const repo = path.dirname(root);
const require = createRequire(import.meta.url);
const demo =
  !live && !explicit
    ? fs.mkdtempSync(path.join(os.tmpdir(), "ccswitch-vibe-"))
    : null;
if (demo)
  process.on("exit", () => fs.rmSync(demo, { recursive: true, force: true }));
const agentDir =
  demo ??
  (explicit
    ? path.resolve(args[1])
    : execFileSync("omp", ["config", "path"], {
        encoding: "utf8",
        timeout: 15_000,
        windowsHide: true,
      }).trim());
if (!path.isAbsolute(agentDir) || !fs.statSync(agentDir).isDirectory())
  throw new Error("不是有效的 OMP agent 目录。");
const store = createStore(agentDir);
if (demo) {
  fs.writeFileSync(
    path.join(demo, "config.yml"),
    `# Isolated Vibe preview\nmodelRoles:\n  default: openai-codex/gpt-5.4\n  smol: zai/glm-4.7-flash\n  task: openai-codex/gpt-5.4:high\ntask:\n  maxEffort: high\n  agentModelOverrides:\n    reviewer: openai-codex/gpt-5.4\n    sonic: "@smol"\n`,
  );
  for (const [name, sonic, task] of [
    ["智谱", "zai/glm-4.7-flash", "zai/glm-5:high"],
    ["GPT", "openai/gpt-5-mini", "openai-codex/gpt-5.4:high"],
    ["智谱 + GPT", "zai/glm-4.7-flash", "openai-codex/gpt-5.4:high"],
  ])
    store.change({
      action: "save",
      name,
      workers: { sonic, task },
      roles: { advisor: task },
      savedRevision: store.get().savedRevision,
    });
}

const listModels = createModelCatalog(agentDir, explicit);
const demoModels = normalizeCatalog({
  models: [
    { provider: "zai", id: "glm-4.7-flash", thinking: ["low", "high"] },
    { provider: "zai", id: "glm-5", thinking: ["low", "high"] },
    {
      provider: "openai",
      id: "gpt-5-mini",
      thinking: ["minimal", "low", "medium", "high"],
    },
    {
      provider: "openai-codex",
      id: "gpt-5.4",
      thinking: ["low", "medium", "high", "xhigh"],
    },
    {
      provider: "deepseek",
      id: "deepseek-v4-flash",
      thinking: ["high", "max"],
    },
    { provider: "deepseek", id: "deepseek-v4-pro", thinking: ["high", "max"] },
  ].map((model) => ({
    ...model,
    kind: "chat",
    name: model.id,
    selector: `${model.provider}/${model.id}`,
  })),
});

const tailwind = require("../tailwind.config.cjs");
const server = await createServer({
  configFile: false,
  root,
  define: { __VIBE_LIVE__: JSON.stringify(!demo) },
  resolve: { alias: { "@": path.join(repo, "src") } },
  css: {
    postcss: {
      plugins: [
        tailwindcss({
          ...tailwind,
          content: [
            ...tailwind.content,
            "./local-omp-vibe-profiles/*.{html,tsx}",
          ],
        }),
        autoprefixer(),
      ],
    },
  },
  server: { host: "127.0.0.1", port: 3317, cors: false, fs: { allow: [repo] } },
  plugins: [
    react(),
    {
      name: "local-vibe-profiles",
      configureServer(server) {
        server.middlewares.use(async (req, res, next) => {
          if (req.url !== "/vibe-profiles" && req.url !== "/vibe-models")
            return next();
          const send = (code, data) => {
            res.writeHead(code, {
              "Content-Type": "application/json; charset=utf-8",
              "Cache-Control": "no-store",
            });
            res.end(JSON.stringify(data));
          };
          const host = `127.0.0.1:${req.socket.localPort}`;
          if (
            req.headers.host !== host ||
            req.headers["sec-fetch-site"] === "cross-site"
          )
            return send(403, { error: "仅允许本机同源预览。" });
          try {
            if (req.url === "/vibe-models") {
              if (req.method !== "GET")
                return send(405, { error: "不支持该请求方法。" });
              return send(200, demo ? demoModels : await listModels());
            }
            if (req.method === "GET") return send(200, store.get());
            if (req.method !== "POST")
              return send(405, { error: "不支持该请求方法。" });
            if (
              req.headers.origin !== `http://${host}` ||
              req.headers["content-type"] !== "application/json"
            )
              return send(403, { error: "仅允许本机同源预览。" });
            let size = 0;
            const chunks = [];
            for await (const chunk of req) {
              size += chunk.length;
              if (size > 32_768) return send(413, { error: "模板过大。" });
              chunks.push(chunk);
            }
            send(
              200,
              store.change(JSON.parse(Buffer.concat(chunks).toString("utf8"))),
            );
          } catch (error) {
            send(400, { error: error.message });
          }
        });
      },
    },
  ],
});
await server.listen();
server.printUrls();
