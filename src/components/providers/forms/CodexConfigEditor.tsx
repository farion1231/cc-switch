import React from "react";
import { CodexAuthSection, CodexConfigSection } from "./CodexConfigSections";

interface CodexConfigEditorProps {
  authValue: string;

  configValue: string;

  onAuthChange: (value: string) => void;

  onConfigChange: (value: string) => void;

  onAuthBlur?: () => void;

  authError: string;

  configError: string;
}

const CodexConfigEditor: React.FC<CodexConfigEditorProps> = ({
  authValue,
  configValue,
  onAuthChange,
  onConfigChange,
  onAuthBlur,
  authError,
  configError,
}) => {
  return (
    <div className="space-y-6">
      {/* Auth JSON Section */}
      <CodexAuthSection
        value={authValue}
        onChange={onAuthChange}
        onBlur={onAuthBlur}
        error={authError}
      />

      {/* Config TOML Section */}
      <CodexConfigSection
        value={configValue}
        onChange={onConfigChange}
        configError={configError}
      />
    </div>
  );
};

export default CodexConfigEditor;
