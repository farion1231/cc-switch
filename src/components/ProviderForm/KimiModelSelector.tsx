import React, { useState, useEffect } from "react";
import { ChevronDown, RefreshCw, AlertCircle } from "lucide-react";

interface KimiModel {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

interface KimiModelSelectorProps {
  apiKey: string;
  anthropicModel: string;
  anthropicSmallFastModel: string;
  onModelChange: (
    field: "ANTHROPIC_MODEL" | "ANTHROPIC_SMALL_FAST_MODEL",
    value: string
  ) => void;
  disabled?: boolean;
}

const KimiModelSelector: React.FC<KimiModelSelectorProps> = ({
  apiKey,
  anthropicModel,
  anthropicSmallFastModel,
  onModelChange,
  disabled = false,
}) => {
  const [models, setModels] = useState<KimiModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  // 获取模型列表
  const fetchModels = async () => {
    if (!apiKey.trim()) {
      setError("请先填写 API Key");
      return;
    }

    setLoading(true);
    setError("");

    try {
      const response = await fetch("https://api.moonshot.cn/v1/models", {
        headers: {
          Authorization: `Bearer ${apiKey.trim()}`,
          "Content-Type": "application/json",
        },
      });

      if (!response.ok) {
        throw new Error(`请求失败: ${response.status} ${response.statusText}`);
      }

      const data = await response.json();

      if (data.data && Array.isArray(data.data)) {
        setModels(data.data);
      } else {
        throw new Error("返回数据格式错误");
      }
    } catch (err) {
      console.error("获取模型列表失败:", err);
      setError(err instanceof Error ? err.message : "获取模型列表失败");
    } finally {
      setLoading(false);
    }
  };

  // 当 API Key 改变时自动获取模型列表
  useEffect(() => {
    if (apiKey.trim()) {
      fetchModels();
    } else {
      setModels([]);
      setError("");
    }
  }, [apiKey]);

  const selectClass = `w-full px-3 py-2 border rounded-lg text-sm transition-colors appearance-none bg-white ${
    disabled
      ? "bg-[var(--color-bg-tertiary)] border-[var(--color-border)] text-[var(--color-text-tertiary)] cursor-not-allowed"
      : "border-[var(--color-border)] focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:border-[var(--color-primary)]"
  }`;

  const ModelSelect: React.FC<{
    label: string;
    value: string;
    onChange: (value: string) => void;
    fieldName: "ANTHROPIC_MODEL" | "ANTHROPIC_SMALL_FAST_MODEL";
  }> = ({ label, value, onChange, fieldName }) => (
    <div className="space-y-2">
      <label className="block text-sm font-medium text-[var(--color-text-primary)]">
        {label}
      </label>
      <div className="relative">
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled || loading || models.length === 0}
          className={selectClass}
        >
          <option value="">
            {loading
              ? "加载中..."
              : models.length === 0
                ? "暂无模型"
                : "请选择模型"}
          </option>
          {models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.id}
            </option>
          ))}
        </select>
        <ChevronDown
          size={16}
          className="absolute right-3 top-1/2 transform -translate-y-1/2 text-[var(--color-text-secondary)] pointer-events-none"
        />
      </div>
    </div>
  );

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-[var(--color-text-primary)]">
          模型配置
        </h3>
        <button
          type="button"
          onClick={fetchModels}
          disabled={disabled || loading || !apiKey.trim()}
          className="inline-flex items-center gap-1 px-2 py-1 text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-primary)] disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          刷新模型列表
        </button>
      </div>

      {error && (
        <div className="flex items-center gap-2 p-3 bg-[var(--color-error-light)] border border-[var(--color-error)]/20 rounded-lg">
          <AlertCircle
            size={16}
            className="text-[var(--color-error)] flex-shrink-0"
          />
          <p className="text-[var(--color-error)] text-xs">{error}</p>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <ModelSelect
          label="主模型 (ANTHROPIC_MODEL)"
          value={anthropicModel}
          onChange={(value) => onModelChange("ANTHROPIC_MODEL", value)}
          fieldName="ANTHROPIC_MODEL"
        />
        <ModelSelect
          label="快速模型 (ANTHROPIC_SMALL_FAST_MODEL)"
          value={anthropicSmallFastModel}
          onChange={(value) =>
            onModelChange("ANTHROPIC_SMALL_FAST_MODEL", value)
          }
          fieldName="ANTHROPIC_SMALL_FAST_MODEL"
        />
      </div>

      {!apiKey.trim() && (
        <p className="text-xs text-[var(--color-text-secondary)]">
          请先填写 API Key 以获取可用模型列表
        </p>
      )}
    </div>
  );
};

export default KimiModelSelector;
