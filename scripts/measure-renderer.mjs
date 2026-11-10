// Run after `pnpm build:renderer --manifest` (or `vite build --manifest`).
// Counts each entry's complete static import graph, including shared chunks.
// Dynamic imports are excluded; this is a load-size metric, not a startup timer.
import { readFile } from "node:fs/promises";
import path from "node:path";
import { gzipSync } from "node:zlib";

const directory = path.resolve(process.argv[2] ?? "dist");
const manifest = JSON.parse(
  await readFile(path.join(directory, ".vite/manifest.json"), "utf8"),
);
const entries = [];
for (const [key, entry] of Object.entries(manifest)) {
  if (!entry.isEntry) continue;
  const visited = new Set();
  const files = new Set();
  function visit(id) {
    if (visited.has(id)) return;
    visited.add(id);
    const chunk = manifest[id];
    if (!chunk) throw new Error(`Missing manifest chunk: ${id}`);
    if (chunk.file.endsWith(".js")) files.add(chunk.file);
    for (const dependency of chunk.imports ?? []) visit(dependency);
  }
  visit(key);
  let bytes = 0;
  let gzipBytes = 0;
  for (const file of files) {
    const source = await readFile(path.join(directory, file));
    bytes += source.length;
    gzipBytes += gzipSync(source).length;
  }
  entries.push({
    entry: key,
    staticJsBytes: bytes,
    gzipBytes,
    files: [...files],
  });
}
console.log(JSON.stringify(entries, null, 2));
