import type { AppId } from "@/lib/api";
import type { Settings } from "@/types";

/**
 * 能改配置目录的应用，一个应用一行，顺序就是设置页里的顺序。
 * 设置字段、恢复默认用的主目录子文件夹、输入框占位文案，以及保存时目录变了
 * 要不要把当前供应商重新写进新目录（syncsLive），都只在这里写一次。
 */
export const DIRECTORY_APPS = [
  {
    app: "claude",
    field: "claudeConfigDir",
    defaultFolder: ".claude",
    placeholderKey: "settings.browsePlaceholderClaude",
    syncsLive: true,
  },
  {
    app: "codex",
    field: "codexConfigDir",
    defaultFolder: ".codex",
    placeholderKey: "settings.browsePlaceholderCodex",
    syncsLive: true,
  },
  {
    app: "gemini",
    field: "geminiConfigDir",
    defaultFolder: ".gemini",
    placeholderKey: "settings.browsePlaceholderGemini",
    syncsLive: true,
  },
  {
    app: "grokbuild",
    field: "grokConfigDir",
    defaultFolder: ".grok",
    placeholderKey: "settings.browsePlaceholderGrok",
    syncsLive: true,
  },
  {
    app: "opencode",
    field: "opencodeConfigDir",
    defaultFolder: ".config/opencode",
    placeholderKey: "settings.browsePlaceholderOpencode",
    syncsLive: true,
  },
  {
    app: "openclaw",
    field: "openclawConfigDir",
    defaultFolder: ".openclaw",
    placeholderKey: "settings.browsePlaceholderOpenclaw",
    syncsLive: true,
  },
  {
    app: "hermes",
    field: "hermesConfigDir",
    defaultFolder: ".hermes",
    placeholderKey: "settings.browsePlaceholderHermes",
    syncsLive: true,
  },
  {
    app: "pi",
    field: "piConfigDir",
    defaultFolder: ".pi/agent",
    placeholderKey: "settings.browsePlaceholderPi",
    syncsLive: false,
  },
  {
    app: "omp",
    field: "ompConfigDir",
    defaultFolder: ".omp/agent",
    placeholderKey: "settings.browsePlaceholderOmp",
    syncsLive: false,
  },
  {
    app: "dsh",
    field: "dshConfigDir",
    defaultFolder: ".dsh",
    placeholderKey: "settings.browsePlaceholderDsh",
    syncsLive: false,
  },
  {
    app: "zcode",
    field: "zcodeConfigDir",
    defaultFolder: ".zcode",
    placeholderKey: "settings.browsePlaceholderZcode",
    syncsLive: true,
  },
  {
    app: "agy",
    field: "agyConfigDir",
    defaultFolder: ".gemini",
    placeholderKey: "settings.browsePlaceholderAgy",
    syncsLive: false,
  },
] as const satisfies readonly {
  app: AppId;
  field: keyof Settings;
  defaultFolder: string;
  placeholderKey: string;
  syncsLive: boolean;
}[];

type DirectoryApp = (typeof DIRECTORY_APPS)[number];
export type DirectoryAppId = DirectoryApp["app"];
export type DirectoryField = DirectoryApp["field"];
export type DirectoryFields = Pick<Settings, DirectoryField>;

export const DIRECTORY_APP = Object.fromEntries(
  DIRECTORY_APPS.map((entry) => [entry.app, entry]),
) as Record<DirectoryAppId, DirectoryApp>;

export const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

/** 每个目录字段去掉首尾空白，空的变成 undefined；键按表的顺序 */
export const sanitizeDirectoryFields = (
  source: Partial<DirectoryFields> | null | undefined,
): DirectoryFields =>
  Object.fromEntries(
    DIRECTORY_APPS.map(({ field }) => [field, sanitizeDir(source?.[field])]),
  ) as DirectoryFields;
