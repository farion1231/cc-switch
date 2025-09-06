import React from "react";

interface CodexConfigEditorProps {
  authValue: string;
  configValue: string;
  onAuthChange: (value: string) => void;
  onConfigChange: (value: string) => void;
  onAuthBlur?: () => void;
}

const CodexConfigEditor: React.FC<CodexConfigEditorProps> = ({
  authValue,
  configValue,
  onAuthChange,
  onConfigChange,
  onAuthBlur,
}) => {
  return (
    <div className="space-y-6">
      <div className="space-y-2">
        <label
          htmlFor="codexAuth"
          className="block text-sm font-medium text-[var(--color-text-primary)]"
        >
          auth.json (JSON) *
        </label>
        <textarea
          id="codexAuth"
          value={authValue}
          onChange={(e) => onAuthChange(e.target.value)}
          onBlur={onAuthBlur}
          placeholder={`{
  "OPENAI_API_KEY": "sk-your-api-key-here"
}`}
          rows={6}
          required
          className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors resize-y min-h-[8rem]"
        />
        <p className="text-xs text-[var(--color-text-secondary)]">
          Codex auth.json 配置内容
        </p>
      </div>

      <div className="space-y-2">
        <label
          htmlFor="codexConfig"
          className="block text-sm font-medium text-[var(--color-text-primary)]"
        >
          config.toml (TOML)
        </label>
        <textarea
          id="codexConfig"
          value={configValue}
          onChange={(e) => onConfigChange(e.target.value)}
          placeholder=""
          rows={8}
          className="w-full px-3 py-2 border border-[var(--color-border)] rounded-lg text-sm font-mono focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)] transition-colors resize-y min-h-[10rem]"
        />
        <p className="text-xs text-[var(--color-text-secondary)]">
          Codex config.toml 配置内容
        </p>
      </div>
    </div>
  );
};

export default CodexConfigEditor;
