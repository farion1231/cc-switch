const tokenRules = [
  ["sk_live_", 24],
  ["sk-", 20],
  ["ghp_", 36],
  ["github_pat_", 82],
  ["xoxb-", 10],
  ["xoxa-", 10],
  ["xoxp-", 10],
  ["xoxr-", 10],
  ["xoxs-", 10],
  ["AKIA", 16],
] as const;

const namedSecrets = ["api_key", "apikey", "token", "secret", "password"];
const unicodeIdentifierCharacter = /[\p{L}\p{N}]/u;
const asciiTokenCharacter = /[A-Za-z0-9_]/;

const hasIdentifierBoundary = (value: string, start: number) => {
  if (start === 0) return true;
  const preceding = Array.from(value.slice(0, start)).at(-1);
  return preceding !== "_" && !unicodeIdentifierCharacter.test(preceding ?? "");
};

const containsToken = (
  value: string,
  prefix: string,
  minimumSuffix: number,
) => {
  let start = value.indexOf(prefix);
  while (start !== -1) {
    if (hasIdentifierBoundary(value, start)) {
      let suffixLength = 0;
      for (const character of value.slice(start + prefix.length)) {
        if (!asciiTokenCharacter.test(character)) break;
        suffixLength += 1;
      }
      if (suffixLength >= minimumSuffix) return true;
    }
    start = value.indexOf(prefix, start + prefix.length);
  }
  return false;
};

const containsPrivateKeyHeader = (value: string) =>
  value.split(/\r?\n/).some((rawLine) => {
    const line = rawLine.trim();
    if (!line.startsWith("-----BEGIN ") || !line.endsWith("PRIVATE KEY-----")) {
      return false;
    }
    const label = line.slice("-----BEGIN ".length, -"PRIVATE KEY-----".length);
    return Array.from(label).every(
      (character) => character === " " || /[A-Z]/.test(character),
    );
  });

const asciiLowercase = (value: string) =>
  value.replace(/[A-Z]/g, (character) => character.toLowerCase());

const containsNamedSecret = (value: string) => {
  const lowercase = asciiLowercase(value);
  return namedSecrets.some((name) => {
    let start = lowercase.indexOf(name);
    while (start !== -1) {
      if (hasIdentifierBoundary(lowercase, start)) {
        const remainder = lowercase.slice(start + name.length).trimStart();
        if (remainder.startsWith(":") || remainder.startsWith("=")) {
          const assigned =
            remainder.slice(1).trimStart().match(/^\S*/u)?.[0] ?? "";
          if (Array.from(assigned).length >= 12) return true;
        }
      }
      start = lowercase.indexOf(name, start + name.length);
    }
    return false;
  });
};

export const containsStructuredCredential = (value: string): boolean =>
  containsPrivateKeyHeader(value) ||
  tokenRules.some(([prefix, minimumSuffix]) =>
    containsToken(value, prefix, minimumSuffix),
  ) ||
  containsNamedSecret(value);
