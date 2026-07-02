#!/usr/bin/env node
/**
 * i18n 同步脚本
 * 从 en.json 读取所有 key，检查其他语言文件是否缺失
 * 缺失的 key 会用英文原文填充（待翻译）
 *
 * 用法: node scripts/sync-i18n.js
 */

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const localesDir = path.join(__dirname, "../src/i18n/locales");

const TARGET_LANGUAGES = ["zh", "zh-TW", "ja"];
const SOURCE_LANG = "en";

function loadJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf-8"));
}

function saveJson(filePath, data) {
  fs.writeFileSync(filePath, JSON.stringify(data, null, 2) + "\n", "utf-8");
}

function getNestedKeys(obj, prefix = "") {
  const keys = [];
  for (const [key, value] of Object.entries(obj)) {
    const fullKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      keys.push(...getNestedKeys(value, fullKey));
    } else {
      keys.push(fullKey);
    }
  }
  return keys;
}

function getNestedValue(obj, keyPath) {
  return keyPath.split(".").reduce((current, key) => current?.[key], obj);
}

function setNestedValue(obj, keyPath, value) {
  const keys = keyPath.split(".");
  let current = obj;
  for (let i = 0; i < keys.length - 1; i++) {
    if (current[keys[i]] == null || typeof current[keys[i]] !== "object") {
      current[keys[i]] = {};
    }
    current = current[keys[i]];
  }
  current[keys[keys.length - 1]] = value;
}

function syncLanguage(sourceData, targetData) {
  const sourceKeys = getNestedKeys(sourceData);
  const targetKeys = getNestedKeys(targetData);

  const targetKeySet = new Set(targetKeys);
  const missingKeys = sourceKeys.filter((key) => !targetKeySet.has(key));

  if (missingKeys.length === 0) {
    return { missingKeys: [], updatedData: targetData };
  }

  const updatedData = { ...targetData };
  for (const key of missingKeys) {
    const sourceValue = getNestedValue(sourceData, key);
    setNestedValue(updatedData, key, sourceValue);
  }

  return { missingKeys, updatedData };
}

// Main
const sourcePath = path.join(localesDir, `${SOURCE_LANG}.json`);
const sourceData = loadJson(sourcePath);
const sourceKeys = getNestedKeys(sourceData);

console.log(`📝 Source: ${SOURCE_LANG}.json (${sourceKeys.length} keys)\n`);

let totalMissing = 0;

for (const lang of TARGET_LANGUAGES) {
  const targetPath = path.join(localesDir, `${lang}.json`);
  const targetData = loadJson(targetPath);
  const { missingKeys, updatedData } = syncLanguage(sourceData, targetData);

  if (missingKeys.length > 0) {
    saveJson(targetPath, updatedData);
    console.log(`✅ ${lang}.json: +${missingKeys.length} keys added`);
    for (const key of missingKeys) {
      console.log(`   - ${key}`);
    }
    totalMissing += missingKeys.length;
  } else {
    console.log(`✅ ${lang}.json: up to date`);
  }
}

console.log(`\n📊 Total: ${totalMissing} keys synced`);
