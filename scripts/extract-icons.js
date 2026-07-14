const fs = require('fs');
const path = require('path');

// 要提取的图标列表（按分类组织）
const ICONS_TO_EXTRACT = {
  // AI 服务商（必需）
  aiProviders: [
    'openai', 'anthropic', 'claude', 'google', 'gemini',
    'deepseek', 'kimi', 'moonshot', 'stepfun', 'zhipu', 'minimax',
    'baidu', 'alibaba', 'tencent', 'meta', 'microsoft',
    'cohere', 'perplexity', 'mistral', 'huggingface'
  ],

  // 云平台
  cloudPlatforms: [
    'aws', 'azure', 'huawei', 'cloudflare'
  ],

  // 开发工具
  devTools: [
    'github', 'gitlab', 'docker', 'kubernetes', 'vscode'
  ],

  // 其他
  others: [
    'settings', 'folder', 'file', 'link'
  ]
};

// 合并所有图标
const ALL_ICONS = [
  ...ICONS_TO_EXTRACT.aiProviders,
  ...ICONS_TO_EXTRACT.cloudPlatforms,
  ...ICONS_TO_EXTRACT.devTools,
  ...ICONS_TO_EXTRACT.others
];

// 提取逻辑
const OUTPUT_DIR = path.join(__dirname, '../src/icons/extracted');
const SOURCE_DIR = path.join(__dirname, '../node_modules/@lobehub/icons-static-svg/icons');

// 确保输出目录存在
if (!fs.existsSync(OUTPUT_DIR)) {
  fs.mkdirSync(OUTPUT_DIR, { recursive: true });
}

console.log('🎨 CC-Switch Icon Extractor\n');
console.log('========================================');
console.log('📦 Extracting icons...\n');

let extracted = 0;
let notFound = [];

// 提取图标
ALL_ICONS.forEach(iconName => {
  const sourceFile = path.join(SOURCE_DIR, `${iconName}.svg`);
  const targetFile = path.join(OUTPUT_DIR, `${iconName}.svg`);

  if (fs.existsSync(sourceFile)) {
    fs.copyFileSync(sourceFile, targetFile);
    console.log(`  ✓ ${iconName}.svg`);
    extracted++;
  } else if (fs.existsSync(targetFile)) {
    console.log(`  ✓ ${iconName}.svg (kept local custom icon)`);
    extracted++;
  } else {
    console.log(`  ✗ ${iconName}.svg (not found)`);
    notFound.push(iconName);
  }
});

// 生成索引文件
console.log('\n📝 Generating index file...\n');

const indexContent = `// Auto-generated icon index
// Do not edit manually

export const icons: Record<string, string> = {
${ALL_ICONS.filter(name => !notFound.includes(name))
  .map(name => {
    const svg = fs.readFileSync(path.join(OUTPUT_DIR, `${name}.svg`), 'utf-8');
    const escaped = svg.replace(/`/g, '\\`').replace(/\$/g, '\\$');
    return `  '${name}': \`${escaped}\`,`;
  })
  .join('\n')}
};

export const iconList = Object.keys(icons);

export function getIcon(name: string): string {
  return icons[name.toLowerCase()] || '';
}

export function hasIcon(name: string): boolean {
  return name.toLowerCase() in icons;
}
`;

fs.writeFileSync(path.join(OUTPUT_DIR, 'index.ts'), indexContent);
console.log('✓ Generated: src/icons/extracted/index.ts');

// 生成图标元数据
const metadataContent = `// Icon metadata for search and categorization
import { IconMetadata } from '@/types/icon';

export const iconMetadata: Record<string, IconMetadata> = {
  // AI Providers
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
  mistral: { name: 'mistral', displayName: 'Mistral', category: 'ai-provider', keywords: ['mistral'], defaultColor: '#FF7000' },
  huggingface: { name: 'huggingface', displayName: 'Hugging Face', category: 'ai-provider', keywords: ['huggingface', 'hf'], defaultColor: '#FFD21E' },

  // Cloud Platforms
  aws: { name: 'aws', displayName: 'AWS', category: 'cloud', keywords: ['amazon', 'cloud'], defaultColor: '#FF9900' },
  azure: { name: 'azure', displayName: 'Azure', category: 'cloud', keywords: ['microsoft', 'cloud'], defaultColor: '#0078D4' },
  huawei: { name: 'huawei', displayName: 'Huawei', category: 'cloud', keywords: ['huawei', 'cloud'], defaultColor: '#FF0000' },
  cloudflare: { name: 'cloudflare', displayName: 'Cloudflare', category: 'cloud', keywords: ['cloudflare', 'cdn'], defaultColor: '#F38020' },

  // Dev Tools
  github: { name: 'github', displayName: 'GitHub', category: 'tool', keywords: ['git', 'version control'], defaultColor: '#181717' },
  gitlab: { name: 'gitlab', displayName: 'GitLab', category: 'tool', keywords: ['git', 'version control'], defaultColor: '#FC6D26' },
  docker: { name: 'docker', displayName: 'Docker', category: 'tool', keywords: ['container'], defaultColor: '#2496ED' },
  kubernetes: { name: 'kubernetes', displayName: 'Kubernetes', category: 'tool', keywords: ['k8s', 'container'], defaultColor: '#326CE5' },
  vscode: { name: 'vscode', displayName: 'VS Code', category: 'tool', keywords: ['editor', 'ide'], defaultColor: '#007ACC' },

  // Others
  settings: { name: 'settings', displayName: 'Settings', category: 'other', keywords: ['config', 'preferences'], defaultColor: '#6B7280' },
  folder: { name: 'folder', displayName: 'Folder', category: 'other', keywords: ['directory'], defaultColor: '#6B7280' },
  file: { name: 'file', displayName: 'File', category: 'other', keywords: ['document'], defaultColor: '#6B7280' },
  link: { name: 'link', displayName: 'Link', category: 'other', keywords: ['url', 'hyperlink'], defaultColor: '#6B7280' },
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

fs.writeFileSync(path.join(OUTPUT_DIR, 'metadata.ts'), metadataContent);
console.log('✓ Generated: src/icons/extracted/metadata.ts');

// 生成 README
const readmeContent = `# Extracted Icons

This directory contains extracted icons from @lobehub/icons-static-svg.

## Statistics
- Total extracted: ${extracted} icons
- Not found: ${notFound.length} icons

## Extracted Icons
${ALL_ICONS.filter(name => !notFound.includes(name)).map(name => `- ${name}`).join('\n')}

${notFound.length > 0 ? `\n## Not Found\n${notFound.map(name => `- ${name}`).join('\n')}` : ''}

## Usage

\`\`\`typescript
import { getIcon, hasIcon, iconList } from './extracted';

// Get icon SVG
const svg = getIcon('openai');

// Check if icon exists
if (hasIcon('openai')) {
  // ...
}

// Get all available icons
console.log(iconList);
\`\`\`

---
Last updated: ${new Date().toISOString()}
Generated by: scripts/extract-icons.js
`;

fs.writeFileSync(path.join(OUTPUT_DIR, 'README.md'), readmeContent);
console.log('✓ Generated: src/icons/extracted/README.md');

console.log('\n========================================');
console.log('✅ Extraction complete!\n');
console.log(`   ✓ Extracted: ${extracted} icons`);
console.log(`   ✗ Not found: ${notFound.length} icons`);
console.log(`   📉 Bundle size reduction: ~${Math.round((1 - extracted / 723) * 100)}%`);
console.log('========================================\n');
