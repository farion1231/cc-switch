export interface PiPromptTemplateSummary {
  description?: string;
  argumentHint?: string;
}

interface FrontmatterDocument {
  lines: string[];
  newline: string;
  closingIndex: number;
}

interface FrontmatterField {
  start: number;
  end: number;
  value: string;
}

const FRONTMATTER_DELIMITER = "---";
const MAX_SUMMARY_LENGTH = 140;
const TOP_LEVEL_FIELD = /^([A-Za-z0-9_-]+)\s*:(.*)$/;

const parseFrontmatter = (content: string): FrontmatterDocument | null => {
  const newline = content.includes("\r\n") ? "\r\n" : "\n";
  const lines = content.split(/\r\n?|\n/);
  if (lines[0]?.trim() !== FRONTMATTER_DELIMITER) return null;

  const relativeClosingIndex = lines
    .slice(1)
    .findIndex((line) => line.trim() === FRONTMATTER_DELIMITER);
  if (relativeClosingIndex < 0) return null;

  return {
    lines,
    newline,
    closingIndex: relativeClosingIndex + 1,
  };
};

const cleanFrontmatterValue = (value: string) => {
  const trimmed = value.trim();
  if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
    try {
      const parsed = JSON.parse(trimmed);
      if (typeof parsed === "string") return parsed.trim();
    } catch {
      return trimmed.slice(1, -1).trim();
    }
  }
  if (trimmed.startsWith("'") && trimmed.endsWith("'")) {
    return trimmed.slice(1, -1).replace(/''/g, "'").trim();
  }
  return trimmed;
};

const findFrontmatterField = (
  document: FrontmatterDocument,
  fieldName: string,
): FrontmatterField | null => {
  for (let index = 1; index < document.closingIndex; index += 1) {
    const match = document.lines[index]?.match(TOP_LEVEL_FIELD);
    if (!match || match[1] !== fieldName) continue;

    const inlineValue = match[2].trim();
    let end = index + 1;
    if (inlineValue === "|" || inlineValue === ">" || !inlineValue) {
      while (
        end < document.closingIndex &&
        (document.lines[end].trim() === "" || /^\s/.test(document.lines[end]))
      ) {
        end += 1;
      }
    }

    const value =
      inlineValue === "|" || inlineValue === ">" || !inlineValue
        ? document.lines
            .slice(index + 1, end)
            .map((line) => line.trim())
            .filter(Boolean)
            .join(inlineValue === "|" ? "\n" : " ")
            .trim()
        : cleanFrontmatterValue(inlineValue);

    return { start: index, end, value };
  }
  return null;
};

const truncateSummary = (value: string) => {
  const compact = value.replace(/\s+/g, " ").trim();
  if (compact.length <= MAX_SUMMARY_LENGTH) return compact;
  return `${compact.slice(0, MAX_SUMMARY_LENGTH - 1).trimEnd()}…`;
};

export function getPiPromptTemplateDescription(
  content: string,
): string | undefined {
  const document = parseFrontmatter(content);
  if (!document) return undefined;
  return findFrontmatterField(document, "description")?.value || undefined;
}

export function stripPiPromptTemplateDescription(content: string): string {
  const document = parseFrontmatter(content);
  if (!document) return content;

  const field = findFrontmatterField(document, "description");
  if (!field) return content;

  const lines = [...document.lines];
  lines.splice(field.start, field.end - field.start);
  const closingIndex = document.closingIndex - (field.end - field.start);
  const hasOtherFrontmatter = lines
    .slice(1, closingIndex)
    .some((line) => line.trim());

  return hasOtherFrontmatter
    ? lines.join(document.newline)
    : lines.slice(closingIndex + 1).join(document.newline);
}

export function setPiPromptTemplateDescription(
  content: string,
  description: string,
): string {
  const newline = content.includes("\r\n") ? "\r\n" : "\n";
  const stripped = stripPiPromptTemplateDescription(content);
  const normalizedDescription = description.trim();
  if (!normalizedDescription) return stripped;

  const descriptionLine = `description: ${JSON.stringify(normalizedDescription)}`;
  const document = parseFrontmatter(stripped);
  if (document) {
    const lines = [...document.lines];
    lines.splice(1, 0, descriptionLine);
    return lines.join(document.newline);
  }

  const prefix = [
    FRONTMATTER_DELIMITER,
    descriptionLine,
    FRONTMATTER_DELIMITER,
  ].join(newline);
  return stripped ? `${prefix}${newline}${stripped}` : `${prefix}${newline}`;
}

export function getPiPromptTemplateSummary(
  content: string,
): PiPromptTemplateSummary {
  const document = parseFrontmatter(content);
  if (!document) return {};

  const description =
    findFrontmatterField(document, "description")?.value || undefined;
  const argumentHint =
    findFrontmatterField(document, "argument-hint")?.value || undefined;

  return {
    description: description ? truncateSummary(description) : undefined,
    argumentHint,
  };
}
