const WINDOWS_RESERVED_BASENAME =
  /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;
const PORTABLE_FILENAME_FORBIDDEN = /[\u0000-\u001f\u007f-\u009f<>:"/\\|?*]/u;

export function isValidPiPromptTemplateSlug(value: string): boolean {
  return (
    value.length > 0 &&
    new TextEncoder().encode(value).byteLength <= 128 &&
    value !== "." &&
    value !== ".." &&
    !value.startsWith(".") &&
    !value.endsWith(".") &&
    !/\s/u.test(value) &&
    !PORTABLE_FILENAME_FORBIDDEN.test(value) &&
    !WINDOWS_RESERVED_BASENAME.test(value)
  );
}
