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
    value: string,
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
  const [debouncedKey, setDebouncedKey] = useState("");

  // 获取模型列表
  const fetchModelsWithKey = async (key: string) => {
    if (!key) {
      setError("请先填写 API Key");
      return;
    }

    setLoading(true);
    setError("");

    try {
      const response = await fetch("https://api.moonshot.cn/v1/models", {
        headers: {
          Authorization: `Bearer ${key}`,
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

  // 500ms 防抖 API Key
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedKey(apiKey.trim());
    }, 500);
    return () => clearTimeout(timer);
  }, [apiKey]);

  // 当防抖后的 Key 改变时自动获取模型列表
  useEffect(() => {
    if (debouncedKey) {
      fetchModelsWithKey(debouncedKey);
    } else {
      setModels([]);
      setError("");
    }
  }, [debouncedKey]);

  const selectClass = `w-full px-3 py-2 border rounded-lg text-sm transition-colors appearance-none bg-white dark:bg-gray-800 ${
    disabled
      ? "bg-gray-100 dark:bg-gray-800 border-gray-200 dark:border-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed"
      : "border-gray-200 dark:border-gray-700 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400"
  }`;

  const ModelSelect: React.FC<{
    label: string;
    value: string;
    onChange: (value: string) => void;
  }> = ({ label, value, onChange }) => (
    <div className="space-y-2">
      <label className="block text-sm font-medium text-gray-900 dark:text-gray-100">
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
          className="absolute right-3 top-1/2 transform -translate-y-1/2 text-gray-500 dark:text-gray-400 pointer-events-none"
        />
      </div>
    </div>
  );

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100">
          模型配置
        </h3>
        <button
          type="button"
          onClick={() => debouncedKey && fetchModelsWithKey(debouncedKey)}
          disabled={disabled || loading || !debouncedKey}
          className="inline-flex items-center gap-1 px-2 py-1 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          刷新模型列表
        </button>
      </div>

      {error && (
        <div className="flex items-center gap-2 p-3 bg-red-100 dark:bg-red-900/20 border border-red-500/20 dark:border-red-500/30 rounded-lg">
          <AlertCircle
            size={16}
            className="text-red-500 dark:text-red-400 flex-shrink-0"
          />
          <p className="text-red-500 dark:text-red-400 text-xs">{error}</p>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <ModelSelect
          label="主模型"
          value={anthropicModel}
          onChange={(value) => onModelChange("ANTHROPIC_MODEL", value)}
        />
        <ModelSelect
          label="快速模型"
          value={anthropicSmallFastModel}
          onChange={(value) =>
            onModelChange("ANTHROPIC_SMALL_FAST_MODEL", value)
          }
        />
      </div>

      {!apiKey.trim() && (
        <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg">
          <p className="text-xs text-amber-600 dark:text-amber-400">
            💡 填写 API Key 后将自动获取可用模型列表
          </p>
        </div>
      )}
    </div>
  );
};

export default KimiModelSelector;
