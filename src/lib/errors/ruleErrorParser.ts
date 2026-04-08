import { TFunction } from "i18next";

export interface RuleError {
  code: string;
  context: Record<string, string>;
  suggestion?: string;
}

export function parseRuleError(errorString: string): RuleError | null {
  try {
    const parsed = JSON.parse(errorString);
    if (parsed.code && parsed.context) {
      return parsed as RuleError;
    }
  } catch {
    // 不是 JSON 格式，返回 null
  }
  return null;
}

function getErrorI18nKey(code: string): string {
  const mapping: Record<string, string> = {
    RULE_NOT_FOUND: "rules.error.ruleNotFound",
    MISSING_REPO_INFO: "rules.error.missingRepoInfo",
    DOWNLOAD_TIMEOUT: "rules.error.downloadTimeout",
    DOWNLOAD_FAILED: "rules.error.downloadFailed",
    RULE_DIR_NOT_FOUND: "rules.error.ruleDirNotFound",
    RULE_DIRECTORY_CONFLICT: "rules.error.directoryConflict",
    EMPTY_ARCHIVE: "rules.error.emptyArchive",
    GET_HOME_DIR_FAILED: "rules.error.getHomeDirFailed",
    NO_RULES_IN_ZIP: "rules.error.noRulesInZip",
  };

  return mapping[code] || "rules.error.unknownError";
}

function getSuggestionI18nKey(suggestion: string): string {
  const mapping: Record<string, string> = {
    checkNetwork: "rules.error.suggestion.checkNetwork",
    checkProxy: "rules.error.suggestion.checkProxy",
    retryLater: "rules.error.suggestion.retryLater",
    checkRepoUrl: "rules.error.suggestion.checkRepoUrl",
    checkPermission: "rules.error.suggestion.checkPermission",
    uninstallFirst: "rules.error.suggestion.uninstallFirst",
    checkZipContent: "rules.error.suggestion.checkZipContent",
    http403: "rules.error.http403",
    http404: "rules.error.http404",
    http429: "rules.error.http429",
  };

  return mapping[suggestion] || suggestion;
}

export function formatRuleError(
  errorString: string,
  t: TFunction,
  defaultTitle: string = "rules.installFailed",
): { title: string; description: string } {
  const parsedError = parseRuleError(errorString);

  if (!parsedError) {
    return {
      title: t(defaultTitle),
      description: errorString || t("common.error"),
    };
  }

  const { code, context, suggestion } = parsedError;

  const errorKey = getErrorI18nKey(code);
  let description = t(errorKey, context);

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
