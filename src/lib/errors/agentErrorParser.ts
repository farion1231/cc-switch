import { TFunction } from "i18next";

export interface AgentError {
  code: string;
  context: Record<string, string>;
  suggestion?: string;
}

export function parseAgentError(errorString: string): AgentError | null {
  try {
    const parsed = JSON.parse(errorString);
    if (parsed.code && parsed.context) {
      return parsed as AgentError;
    }
  } catch {
    // 不是 JSON 格式，返回 null
  }
  return null;
}

function getErrorI18nKey(code: string): string {
  const mapping: Record<string, string> = {
    AGENT_NOT_FOUND: "agents.error.agentNotFound",
    MISSING_REPO_INFO: "agents.error.missingRepoInfo",
    DOWNLOAD_TIMEOUT: "agents.error.downloadTimeout",
    DOWNLOAD_FAILED: "agents.error.downloadFailed",
    AGENT_DIR_NOT_FOUND: "agents.error.agentDirNotFound",
    AGENT_DIRECTORY_CONFLICT: "agents.error.directoryConflict",
    EMPTY_ARCHIVE: "agents.error.emptyArchive",
    GET_HOME_DIR_FAILED: "agents.error.getHomeDirFailed",
    NO_AGENTS_IN_ZIP: "agents.error.noAgentsInZip",
  };

  return mapping[code] || "agents.error.unknownError";
}

function getSuggestionI18nKey(suggestion: string): string {
  const mapping: Record<string, string> = {
    checkNetwork: "agents.error.suggestion.checkNetwork",
    checkProxy: "agents.error.suggestion.checkProxy",
    retryLater: "agents.error.suggestion.retryLater",
    checkRepoUrl: "agents.error.suggestion.checkRepoUrl",
    checkPermission: "agents.error.suggestion.checkPermission",
    uninstallFirst: "agents.error.suggestion.uninstallFirst",
    checkZipContent: "agents.error.suggestion.checkZipContent",
    http403: "agents.error.http403",
    http404: "agents.error.http404",
    http429: "agents.error.http429",
  };

  return mapping[suggestion] || suggestion;
}

export function formatAgentError(
  errorString: string,
  t: TFunction,
  defaultTitle: string = "agents.installFailed",
): { title: string; description: string } {
  const parsedError = parseAgentError(errorString);

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
