import { TFunction } from "i18next";

/**
 * 结构化错误对象
 */
export interface SkillError {
  code: string;
  context: Record<string, string>;
  suggestion?: string;
}

/**
 * 尝试解析后端返回的错误字符串
 * 如果是 JSON 格式，返回结构化错误；否则返回 null
 */
export function parseSkillError(errorString: string): SkillError | null {
  try {
    const parsed = JSON.parse(errorString);
    if (parsed.code && parsed.context) {
      return parsed as SkillError;
    }
  } catch {
    // 不是 JSON 格式，返回 null
  }
  return null;
}

/**
 * 将错误码映射到 i18n key
 */
function getErrorI18nKey(code: string): string {
  const mapping: Record<string, string> = {
    SKILL_NOT_FOUND: "skills.error.skillNotFound",
    MISSING_REPO_INFO: "skills.error.missingRepoInfo",
    DOWNLOAD_TIMEOUT: "skills.error.downloadTimeout",
    DOWNLOAD_FAILED: "skills.error.downloadFailed",
    SKILL_DIR_NOT_FOUND: "skills.error.skillDirNotFound",
    EMPTY_ARCHIVE: "skills.error.emptyArchive",
    GET_HOME_DIR_FAILED: "skills.error.getHomeDirFailed",
  };

  return mapping[code] || "skills.error.unknownError";
}

/**
 * 将建议码映射到 i18n key
 */
function getSuggestionI18nKey(suggestion: string): string {
  const mapping: Record<string, string> = {
    checkNetwork: "skills.error.suggestion.checkNetwork",
    checkProxy: "skills.error.suggestion.checkProxy",
    retryLater: "skills.error.suggestion.retryLater",
    checkRepoUrl: "skills.error.suggestion.checkRepoUrl",
    checkPermission: "skills.error.suggestion.checkPermission",
    http403: "skills.error.http403",
    http404: "skills.error.http404",
    http429: "skills.error.http429",
  };

  return mapping[suggestion] || suggestion;
}

/**
 * 格式化技能错误为用户友好的消息
 * @param errorString 后端返回的错误字符串
 * @param t i18next 翻译函数
 * @param defaultTitle 默认标题的 i18n key（如 "skills.installFailed"）
 * @returns 包含标题和描述的对象
 */
export function formatSkillError(
  errorString: string,
  t: TFunction,
  defaultTitle: string = "skills.installFailed",
): { title: string; description: string } {
  const parsedError = parseSkillError(errorString);

  if (!parsedError) {
    // 如果不是结构化错误，返回原始错误字符串
    return {
      title: t(defaultTitle),
      description: errorString || t("common.error"),
    };
  }

  const { code, context, suggestion } = parsedError;

  // 获取错误消息的 i18n key
  const errorKey = getErrorI18nKey(code);

  // 构建描述（错误消息 + 建议）
  let description = t(errorKey, context);

  // 如果有建议，追加到描述中
  if (suggestion) {
    const suggestionKey = getSuggestionI18nKey(suggestion);
    const suggestionText = t(suggestionKey);
    description += `\n\n${suggestionText}`;
  }

  return {
    title: t(defaultTitle),
    description,
  };
}
