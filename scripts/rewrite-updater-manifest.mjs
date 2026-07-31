#!/usr/bin/env node
// Rewrites the Tauri updater manifest (latest.json) so its download URLs point
// at the R2 mirror instead of GitHub Releases. Minisign signatures cover file
// contents, not URLs, so they stay valid unchanged — the updater still verifies
// every downloaded artifact against the pubkey baked into the app.
//
// Usage: node scripts/rewrite-updater-manifest.mjs <latest-json> <tag> <base-url> [output]

import { readFileSync, writeFileSync } from 'node:fs';

const [input, tag, baseUrl, output = 'latest-r2.json'] = process.argv.slice(2);

if (!input || !tag || !baseUrl) {
  console.error('Usage: node scripts/rewrite-updater-manifest.mjs <latest-json> <tag> <base-url> [output]');
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(input, 'utf8'));
const platforms = Object.entries(manifest.platforms ?? {});
if (platforms.length === 0) {
  console.error(`No platforms found in ${input}`);
  process.exit(1);
}

const normalizedBase = baseUrl.replace(/\/+$/, '');
// Mirrored artifacts live at <base>/<tag>/<file>; on GitHub they live at
// .../releases/download/<tag>/<file>. Rebuild from the filename rather than
// prefix-replacing so a layout change on either side fails loudly here.
const githubPrefix = `https://github.com/`;

for (const [key, entry] of platforms) {
  if (typeof entry?.url !== 'string' || typeof entry?.signature !== 'string') {
    console.error(`Platform ${key} is missing url or signature`);
    process.exit(1);
  }
  if (!entry.url.startsWith(githubPrefix) || !entry.url.includes(`/releases/download/${tag}/`)) {
    console.error(`Platform ${key} has unexpected url for ${tag}: ${entry.url}`);
    process.exit(1);
  }
  const name = entry.url.split('/').pop();
  entry.url = `${normalizedBase}/${tag}/${name}`;
}

writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote ${output} with ${platforms.length} platforms pointing at ${normalizedBase}/${tag}/`);
