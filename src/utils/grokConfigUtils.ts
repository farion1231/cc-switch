import { parse as parseToml } from "smol-toml";
import { normalizeTomlText } from "@/utils/textNormalization";

type TomlObject = Record<string, unknown>;

function parseGrokConfig(config: string): TomlObject | null {
  try {
    return parseToml(normalizeTomlText(config)) as TomlObject;
  } catch {
    return null;
  }
}

export function extractGrokBaseUrl(config: string): string {
  const parsed = parseGrokConfig(config);
  const endpoints = parsed?.endpoints as TomlObject | undefined;
  if (typeof endpoints?.models_base_url === "string") {
    return endpoints.models_base_url;
  }
  const models = parsed?.models as TomlObject | undefined;
  const modelRoot = parsed?.model as TomlObject | undefined;
  const selected =
    typeof models?.default === "string" ? models.default : undefined;
  const selectedModel = selected
    ? (modelRoot?.[selected] as TomlObject | undefined)
    : undefined;
  return typeof selectedModel?.base_url === "string"
    ? selectedModel.base_url
    : "";
}

export function extractGrokApiBackend(config: string): string | undefined {
  const parsed = parseGrokConfig(config);
  const models = parsed?.models as TomlObject | undefined;
  const modelRoot = parsed?.model as TomlObject | undefined;
  const selected =
    typeof models?.default === "string" ? models.default : undefined;
  const selectedModel = selected
    ? (modelRoot?.[selected] as TomlObject | undefined)
    : undefined;
  return typeof selectedModel?.api_backend === "string"
    ? selectedModel.api_backend
    : undefined;
}

function setSectionString(
  config: string,
  section: string,
  key: string,
  nextValue: string,
): string {
  const normalized = normalizeTomlText(config).replace(/\r\n/g, "\n");
  const lines = normalized ? normalized.split("\n") : [];
  const sectionHeader = `[${section}]`;
  const sectionStart = lines.findIndex((line) => line.trim() === sectionHeader);
  const assignment = `${key} = ${JSON.stringify(nextValue)}`;

  if (sectionStart < 0) {
    if (!nextValue) return normalized;
    const prefix = normalized.trimEnd();
    return `${prefix}${prefix ? "\n\n" : ""}${sectionHeader}\n${assignment}\n`;
  }

  let sectionEnd = lines.length;
  for (let index = sectionStart + 1; index < lines.length; index += 1) {
    if (/^\s*\[[^\]]+\]\s*$/.test(lines[index])) {
      sectionEnd = index;
      break;
    }
  }
  const keyPattern = new RegExp(`^\\s*${key}\\s*=`);
  const keyIndex = lines
    .slice(sectionStart + 1, sectionEnd)
    .findIndex((line) => keyPattern.test(line));
  if (keyIndex >= 0) {
    const absoluteIndex = sectionStart + 1 + keyIndex;
    if (nextValue) lines[absoluteIndex] = assignment;
    else lines.splice(absoluteIndex, 1);
  } else if (nextValue) {
    lines.splice(sectionStart + 1, 0, assignment);
  }
  return `${lines.join("\n").trimEnd()}\n`;
}

export function setGrokBaseUrl(config: string, baseUrl: string): string {
  return setSectionString(
    config,
    "endpoints",
    "models_base_url",
    baseUrl.trim(),
  );
}

export function setGrokApiBackend(
  config: string,
  backend: "responses" | "chat_completions",
): string {
  const lines = normalizeTomlText(config).replace(/\r\n/g, "\n").split("\n");
  const starts: number[] = [];
  lines.forEach((line, index) => {
    if (/^\s*\[model\.[^\]]+\]\s*$/.test(line)) starts.push(index);
  });
  for (let offset = starts.length - 1; offset >= 0; offset -= 1) {
    const start = starts[offset];
    const end =
      lines.findIndex(
        (line, index) => index > start && /^\s*\[[^\]]+\]\s*$/.test(line),
      ) || lines.length;
    const actualEnd = end < 0 ? lines.length : end;
    const keyIndex = lines
      .slice(start + 1, actualEnd)
      .findIndex((line) => /^\s*api_backend\s*=/.test(line));
    const assignment = `api_backend = ${JSON.stringify(backend)}`;
    if (keyIndex >= 0) lines[start + 1 + keyIndex] = assignment;
    else lines.splice(start + 1, 0, assignment);
  }
  return `${lines.join("\n").trimEnd()}\n`;
}
