const fs = require('fs');
const path = require('path');

const ICONS_DIR = path.join(__dirname, '../src/icons/extracted');

// List of "Famous" icons to keep
// Based on common AI providers and tools
const KEEP_LIST = [
    // AI Providers
    'openai', 'anthropic', 'claude', 'google', 'gemini', 'gemma', 'palm',
    'microsoft', 'azure', 'copilot', 'meta', 'llama',
    'alibaba', 'qwen', 'tencent', 'hunyuan', 'baidu', 'wenxin',
    'bytedance', 'doubao', 'deepseek', 'moonshot', 'kimi',
    'zhipu', 'chatglm', 'glm', 'minimax', 'mistral', 'cohere',
    'perplexity', 'huggingface', 'midjourney', 'stability',
    'xai', 'grok', 'yi', 'zeroone', 'ollama',
    'packycode',

    // Cloud/Tools
    'aws', 'googlecloud', 'huawei', 'cloudflare',
    'github', 'githubcopilot', 'vercel', 'notion', 'discord',
    'gitlab', 'docker', 'kubernetes', 'vscode', 'settings', 'folder', 'file', 'link'
];

// Get all SVG files
const files = fs.readdirSync(ICONS_DIR).filter(file => file.endsWith('.svg'));

console.log(`Scanning ${files.length} files...`);

let keptCount = 0;
let deletedCount = 0;
let renamedCount = 0;

// First pass: Identify files to keep and prefer color versions
const fileMap = {}; // name -> { hasColor: bool, hasMono: bool }

files.forEach(file => {
    const isColor = file.endsWith('-color.svg');
    const baseName = isColor ? file.replace('-color.svg', '') : file.replace('.svg', '');

    if (!fileMap[baseName]) {
        fileMap[baseName] = { hasColor: false, hasMono: false };
    }

    if (isColor) {
        fileMap[baseName].hasColor = true;
    } else {
        fileMap[baseName].hasMono = true;
    }
});

// Second pass: Process files
Object.keys(fileMap).forEach(baseName => {
    const info = fileMap[baseName];
    const shouldKeep = KEEP_LIST.includes(baseName);

    if (!shouldKeep) {
        // Delete both versions if not in keep list
        if (info.hasColor) {
            fs.unlinkSync(path.join(ICONS_DIR, `${baseName}-color.svg`));
            deletedCount++;
        }
        if (info.hasMono) {
            fs.unlinkSync(path.join(ICONS_DIR, `${baseName}.svg`));
            deletedCount++;
        }
        return;
    }

    // If keeping, prefer color
    if (info.hasColor) {
        // Rename color version to base version (overwrite mono if exists)
        const colorPath = path.join(ICONS_DIR, `${baseName}-color.svg`);
        const targetPath = path.join(ICONS_DIR, `${baseName}.svg`);

        try {
            // If mono exists, it will be overwritten/replaced
            fs.renameSync(colorPath, targetPath);
            renamedCount++;
            keptCount++;
        } catch (e) {
            console.error(`Error renaming ${baseName}:`, e);
        }
    } else if (info.hasMono) {
        // Keep mono if no color version
        keptCount++;
    }
});

console.log(`\nCleanup complete:`);
console.log(`- Kept: ${keptCount}`);
console.log(`- Deleted: ${deletedCount}`);
console.log(`- Renamed (Color -> Standard): ${renamedCount}`);

// Regenerate index and metadata
require('./generate-icon-index.js');
