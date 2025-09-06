import React from "react";
import JsonEditor from "../JsonEditor";

interface ClaudeConfigEditorProps {
  value: string;
  onChange: (value: string) => void;
  disableCoAuthored: boolean;
  onCoAuthoredToggle: (checked: boolean) => void;
}

const ClaudeConfigEditor: React.FC<ClaudeConfigEditorProps> = ({
  value,
  onChange,
  disableCoAuthored,
  onCoAuthoredToggle,
}) => {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label
          htmlFor="settingsConfig"
          className="block text-sm font-medium text-[var(--color-text-primary)]"
        >
          Claude Code 配置 (JSON) *
        </label>
        <label className="inline-flex items-center gap-2 text-sm text-[var(--color-text-secondary)] cursor-pointer">
          <input
            type="checkbox"
            checked={disableCoAuthored}
            onChange={(e) => onCoAuthoredToggle(e.target.checked)}
            className="w-4 h-4 text-[var(--color-primary)] bg-white border-[var(--color-border)] rounded focus:ring-[var(--color-primary)] focus:ring-2"
          />
          禁止 Claude Code 签名
        </label>
      </div>
      <JsonEditor
        value={value}
        onChange={onChange}
        placeholder={`{
  "env": {
    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-your-api-key-here"
  }
}`}
        rows={12}
      />
      <p className="text-xs text-[var(--color-text-secondary)]">
        完整的 Claude Code settings.json 配置内容
      </p>
    </div>
  );
};

export default ClaudeConfigEditor;
