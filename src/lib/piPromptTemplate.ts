export interface PiPromptTemplateSummary {
  description?: string;
  argumentHint?: string;
}

const FRONTMATTER_DELIMITER = "---";
const MAX_SUMMARY_LENGTH = 140;

const cleanFrontmatterValue = (value: string) => {
  const trimmed = value.trim();
  if (
    trimmed.length >= 2 &&
    ((trimmed.startsWith('"') && trimmed.endsWith('"')) ||
      (trimmed.startsWith("'") && trimmed.endsWith("'")))
  ) {
    return trimmed.slice(1, -1).trim();
  }
  return trimmed;
};

const truncateSummary = (value: string) => {
  const compact = value.replace(/\s+/g, " ").trim();
  if (compact.length <= MAX_SUMMARY_LENGTH) return compact;
  return `${compact.slice(0, MAX_SUMMARY_LENGTH - 1).trimEnd()}…`;
};

export function getPiPromptTemplateSummary(
  content: string,
): PiPromptTemplateSummary {
  const lines = content.replace(/\r\n?/g, "\n").split("\n");
  let bodyStart = 0;
  let description: string | undefined;
  let argumentHint: string | undefined;

  if (lines[0]?.trim() === FRONTMATTER_DELIMITER) {
    const closingIndex = lines
      .slice(1)
      .findIndex((line) => line.trim() === FRONTMATTER_DELIMITER);

    if (closingIndex >= 0) {
      const frontmatterEnd = closingIndex + 1;
      for (const line of lines.slice(1, frontmatterEnd)) {
        const separatorIndex = line.indexOf(":");
        if (separatorIndex < 0) continue;
        const key = line.slice(0, separatorIndex).trim();
        const value = cleanFrontmatterValue(line.slice(separatorIndex + 1));
        if (!value) continue;
        if (key === "description") description = value;
        if (key === "argument-hint") argumentHint = value;
      }
      bodyStart = frontmatterEnd + 1;
    }
  }

  const fallbackDescription = lines
    .slice(bodyStart)
    .map((line) => line.trim())
    .find(Boolean);

  return {
    description: description
      ? truncateSummary(description)
      : fallbackDescription
        ? truncateSummary(fallbackDescription)
        : undefined,
    argumentHint,
  };
}
