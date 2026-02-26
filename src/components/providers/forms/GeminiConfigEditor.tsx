import React from "react";
import { GeminiEnvSection, GeminiConfigSection } from "./GeminiConfigSections";

interface GeminiConfigEditorProps {
  envValue: string;
  configValue: string;
  onEnvChange: (value: string) => void;
  onConfigChange: (value: string) => void;
  onEnvBlur?: () => void;
  envError: string;
  configError: string;
}

const GeminiConfigEditor: React.FC<GeminiConfigEditorProps> = ({
  envValue,
  configValue,
  onEnvChange,
  onConfigChange,
  onEnvBlur,
  envError,
  configError,
}) => {
  return (
    <div className="space-y-6">
      {/* Env Section */}
      <GeminiEnvSection
        value={envValue}
        onChange={onEnvChange}
        onBlur={onEnvBlur}
        error={envError}
      />

      {/* Config JSON Section */}
      <GeminiConfigSection
        value={configValue}
        onChange={onConfigChange}
        configError={configError}
      />
    </div>
  );
};

export default GeminiConfigEditor;
