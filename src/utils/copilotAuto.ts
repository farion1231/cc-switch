export function isCopilotAutoModel(model: string): boolean {
  return (
    model
      .trim()
      .replace(/\[1m\]$/i, "")
      .trim()
      .toLowerCase() === "copilot-auto"
  );
}
