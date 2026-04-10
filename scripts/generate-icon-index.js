import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const ICONS_DIR = path.join(__dirname, '../src/icons/extracted');
const INDEX_FILE = path.join(ICONS_DIR, 'index.ts');
const METADATA_FILE = path.join(ICONS_DIR, 'metadata.ts');

// Supported image extensions
const SUPPORTED_EXTENSIONS = ['.svg', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.ico'];

// ── Manual render mode control ──────────────────────────────────────
// SVG icons listed here will be imported as URLs and rendered via <img>.
// All other SVGs will be inlined as strings and rendered via dangerouslySetInnerHTML.
// Raster images (png/jpg/…) are always URL-based regardless of this list.
//
// Add an icon name here when:
//   - The SVG file is too large to inline (e.g. > 100 KB)
//   - The SVG doesn't render correctly when inlined in HTML
const URL_ICONS = new Set([
  'dds',
]);
// ─────────────────────────────────────────────────────────────────────

// Known metadata from previous configuration
const KNOWN_METADATA = {
  openai: { name: 'openai', displayName: 'OpenAI', category: 'ai-provider', keywords: ['gpt', 'chatgpt'], defaultColor: '#00A67E' },
  anthropic: { name: 'anthropic', displayName: 'Anthropic', category: 'ai-provider', keywords: ['claude'], defaultColor: '#D4915D' },
  claude: { name: 'claude', displayName: 'Claude', category: 'ai-provider', keywords: ['anthropic'], defaultColor: '#D4915D' },
  google: { name: 'google', displayName: 'Google', category: 'ai-provider', keywords: ['gemini', 'bard'], defaultColor: '#4285F4' },
  gemini: { name: 'gemini', displayName: 'Gemini', category: 'ai-provider', keywords: ['google'], defaultColor: '#4285F4' },
  deepseek: { name: 'deepseek', displayName: 'DeepSeek', category: 'ai-provider', keywords: ['deep', 'seek'], defaultColor: '#1E88E5' },
  moonshot: { name: 'moonshot', displayName: 'Moonshot', category: 'ai-provider', keywords: ['kimi', 'moonshot'], defaultColor: '#6366F1' },
  kimi: { name: 'kimi', displayName: 'Kimi', category: 'ai-provider', keywords: ['moonshot'], defaultColor: '#6366F1' },
  stepfun: { name: 'stepfun', displayName: 'StepFun', category: 'ai-provider', keywords: ['stepfun', 'step', 'jieyue', '阶跃星辰'], defaultColor: '#005AFF' },
  zhipu: { name: 'zhipu', displayName: 'Zhipu AI', category: 'ai-provider', keywords: ['chatglm', 'glm'], defaultColor: '#0F62FE' },
  minimax: { name: 'minimax', displayName: 'MiniMax', category: 'ai-provider', keywords: ['minimax'], defaultColor: '#FF6B6B' },
  baidu: { name: 'baidu', displayName: 'Baidu', category: 'ai-provider', keywords: ['ernie', 'wenxin'], defaultColor: '#2932E1' },
  alibaba: { name: 'alibaba', displayName: 'Alibaba', category: 'ai-provider', keywords: ['qwen', 'tongyi'], defaultColor: '#FF6A00' },
  tencent: { name: 'tencent', displayName: 'Tencent', category: 'ai-provider', keywords: ['hunyuan'], defaultColor: '#00A4FF' },
  meta: { name: 'meta', displayName: 'Meta', category: 'ai-provider', keywords: ['facebook', 'llama'], defaultColor: '#0081FB' },
  microsoft: { name: 'microsoft', displayName: 'Microsoft', category: 'ai-provider', keywords: ['copilot', 'azure'], defaultColor: '#00A4EF' },
  cohere: { name: 'cohere', displayName: 'Cohere', category: 'ai-provider', keywords: ['cohere'], defaultColor: '#39594D' },
  perplexity: { name: 'perplexity', displayName: 'Perplexity', category: 'ai-provider', keywords: ['perplexity'], defaultColor: '#20808D' },
  packycode: { name: 'packycode', displayName: 'PackyCode', category: 'ai-provider', keywords: ['packycode', 'packy', 'packyapi'], defaultColor: 'currentColor' },
  mistral: { name: 'mistral', displayName: 'Mistral', category: 'ai-provider', keywords: ['mistral'], defaultColor: '#FF7000' },
  huggingface: { name: 'huggingface', displayName: 'Hugging Face', category: 'ai-provider', keywords: ['huggingface', 'hf'], defaultColor: '#FFD21E' },
  aws: { name: 'aws', displayName: 'AWS', category: 'cloud', keywords: ['amazon', 'cloud'], defaultColor: '#FF9900' },
  azure: { name: 'azure', displayName: 'Azure', category: 'cloud', keywords: ['microsoft', 'cloud'], defaultColor: '#0078D4' },
  huawei: { name: 'huawei', displayName: 'Huawei', category: 'cloud', keywords: ['huawei', 'cloud'], defaultColor: '#FF0000' },
  cloudflare: { name: 'cloudflare', displayName: 'Cloudflare', category: 'cloud', keywords: ['cloudflare', 'cdn'], defaultColor: '#F38020' },
  github: { name: 'github', displayName: 'GitHub', category: 'tool', keywords: ['git', 'version control'], defaultColor: '#181717' },
  gitlab: { name: 'gitlab', displayName: 'GitLab', category: 'tool', keywords: ['git', 'version control'], defaultColor: '#FC6D26' },
  docker: { name: 'docker', displayName: 'Docker', category: 'tool', keywords: ['container'], defaultColor: '#2496ED' },
  kubernetes: { name: 'kubernetes', displayName: 'Kubernetes', category: 'tool', keywords: ['k8s', 'container'], defaultColor: '#326CE5' },
  vscode: { name: 'vscode', displayName: 'VS Code', category: 'tool', keywords: ['editor', 'ide'], defaultColor: '#007ACC' },
  settings: { name: 'settings', displayName: 'Settings', category: 'other', keywords: ['config', 'preferences'], defaultColor: '#6B7280' },
  folder: { name: 'folder', displayName: 'Folder', category: 'other', keywords: ['directory'], defaultColor: '#6B7280' },
  file: { name: 'file', displayName: 'File', category: 'other', keywords: ['document'], defaultColor: '#6B7280' },
  link: { name: 'link', displayName: 'Link', category: 'other', keywords: ['url', 'hyperlink'], defaultColor: '#6B7280' },
};

// Sanitize a filename into a valid JS identifier for import variable names
function toImportVar(name) {
  return '_' + name.replace(/[^a-zA-Z0-9_]/g, '_');
}

// Strip XML declarations and DOCTYPE from SVG content for safe HTML embedding
function cleanSvgForInline(svg) {
  return svg
    .replace(/<\?xml[^?]*\?>\s*/g, '')
    .replace(/<!DOCTYPE[^>]*>\s*/g, '')
    .trim();
}

// Get all supported image files
const files = fs.readdirSync(ICONS_DIR).filter(file =>
  SUPPORTED_EXTENSIONS.includes(path.extname(file).toLowerCase())
);

console.log(`Found ${files.length} icon files.`);

// Classify files
const inlineFiles = [];  // SVGs to inline as strings (dangerouslySetInnerHTML)
const urlFiles = [];     // SVGs/raster to import as URLs (<img>)
const seenNames = new Map();

for (const file of files) {
  const ext = path.extname(file).toLowerCase();
  const name = path.basename(file, path.extname(file)).toLowerCase();

  // Duplicate name detection: prefer SVG over raster
  if (seenNames.has(name)) {
    const existing = seenNames.get(name);
    const existingExt = path.extname(existing).toLowerCase();
    if (ext === '.svg' && existingExt !== '.svg') {
      console.warn(`Warning: duplicate icon name "${name}" — ${file} (SVG) replaces ${existing}`);
      inlineFiles.splice(inlineFiles.indexOf(existing), 1);
      urlFiles.splice(urlFiles.indexOf(existing), 1);
    } else {
      console.warn(`Warning: duplicate icon name "${name}" — skipping ${file}, keeping ${existing}`);
      continue;
    }
  }
  seenNames.set(name, file);

  if (ext === '.svg' && !URL_ICONS.has(name)) {
    inlineFiles.push(file);
  } else {
    urlFiles.push(file);
    const reason = ext !== '.svg' ? 'raster' : 'listed in URL_ICONS';
    console.log(`  URL import (${reason}): ${file}`);
  }
}

console.log(`  Inline SVGs: ${inlineFiles.length}, URL-based: ${urlFiles.length}`);

// ── Generate index.ts ──

const urlImports = urlFiles.map(file => {
  const ext = path.extname(file).toLowerCase();
  const name = path.basename(file, path.extname(file)).toLowerCase();
  const varName = toImportVar(name);
  const importSuffix = ext === '.svg' ? '?url' : '';
  return `import ${varName} from './${file}${importSuffix}';`;
}).join('\n');

const inlineEntries = inlineFiles.map(file => {
  const name = path.basename(file, '.svg').toLowerCase();
  const raw = fs.readFileSync(path.join(ICONS_DIR, file), 'utf-8');
  const svg = cleanSvgForInline(raw);
  const escaped = svg.replace(/`/g, '\\`').replace(/\$/g, '\\$');
  return `  '${name}': \`${escaped}\`,`;
}).join('\n');

const urlEntries = urlFiles.map(file => {
  const name = path.basename(file, path.extname(file)).toLowerCase();
  const varName = toImportVar(name);
  return `  '${name}': ${varName},`;
}).join('\n');

const indexContent = `// Auto-generated icon index
// Do not edit manually
${urlImports ? '\n' + urlImports + '\n' : ''}
export const icons: Record<string, string> = {
${inlineEntries}
};

export const iconUrls: Record<string, string> = {
${urlEntries}
};

export const iconList = [...Object.keys(icons), ...Object.keys(iconUrls)].sort();

export function getIcon(name: string): string {
  return icons[name.toLowerCase()] || '';
}

export function getIconUrl(name: string): string {
  return iconUrls[name.toLowerCase()] || '';
}

export function hasIcon(name: string): boolean {
  const key = name.toLowerCase();
  return key in icons || key in iconUrls;
}

export function isUrlIcon(name: string): boolean {
  return name.toLowerCase() in iconUrls;
}

export { getIconMetadata } from './metadata';
`;

fs.writeFileSync(INDEX_FILE, indexContent);
console.log(`Generated ${INDEX_FILE} (inline: ${inlineFiles.length}, url: ${urlFiles.length})`);

// ── Generate metadata.ts ──

const allFiles = [...inlineFiles, ...urlFiles];
const metadataEntries = allFiles.map(file => {
  const ext = path.extname(file);
  const name = path.basename(file, ext).toLowerCase();
  const known = KNOWN_METADATA[name];

  if (known) {
    return `  ${name}: ${JSON.stringify(known)},`;
  }

  return `  '${name}': { name: '${name}', displayName: '${name}', category: 'other', keywords: [], defaultColor: 'currentColor' },`;
});

const metadataContent = `// Icon metadata for search and categorization
import { IconMetadata } from '@/types/icon';

export const iconMetadata: Record<string, IconMetadata> = {
${metadataEntries.join('\n')}
};

export function getIconMetadata(name: string): IconMetadata | undefined {
  return iconMetadata[name.toLowerCase()];
}

export function searchIcons(query: string): string[] {
  const lowerQuery = query.toLowerCase();
  return Object.values(iconMetadata)
    .filter(meta =>
      meta.name.includes(lowerQuery) ||
      meta.displayName.toLowerCase().includes(lowerQuery) ||
      meta.keywords.some(k => k.includes(lowerQuery))
    )
    .map(meta => meta.name);
}
`;

fs.writeFileSync(METADATA_FILE, metadataContent);
console.log(`Generated ${METADATA_FILE}`);
