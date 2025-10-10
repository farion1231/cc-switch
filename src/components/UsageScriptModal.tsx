import React, { useState } from "react";
import { X, Play, Wand2 } from "lucide-react";
import { Provider, UsageScript } from "../types";
import { AppType } from "../lib/tauri-api";
import JsonEditor from "./JsonEditor";
import * as prettier from "prettier/standalone";
import * as parserBabel from "prettier/parser-babel";
import * as pluginEstree from "prettier/plugins/estree";

interface UsageScriptModalProps {
  provider: Provider;
  appType: AppType;
  onClose: () => void;
  onSave: (script: UsageScript) => void;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number
  ) => void;
}

// 预设模板（JS 对象字面量格式）
const PRESET_TEMPLATES: Record<string, string> = {
  通用模板: `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    }
  },
  extractor: function(response) {
    return {
      isValid: response.is_active || true,
      remaining: response.balance,
      unit: "USD"
    };
  }
})`,

  NewAPI: `({
  request: {
    url: "{{baseUrl}}/api/usage/token",
    method: "GET",
    headers: {
      Authorization: "Bearer {{apiKey}}",
    },
  },
  extractor: function (response) {
    if (response.code) {
      if (response.data.unlimited_quota) {
        return {
          planName: response.data.name,
          total: -1,
          used: response.data.total_used / 500000,
          unit: "USD",
        };
      }
      return {
        isValid: true,
        planName: response.data.name,
        total: response.data.total_granted / 500000,
        used: response.data.total_used / 500000,
        remaining: response.data.total_available / 500000,
        unit: "USD",
      };
    }
    if (response.error) {
      return {
        isValid: false,
        invalidMessage: response.error.message,
      };
    }
  },
})`,
};

const UsageScriptModal: React.FC<UsageScriptModalProps> = ({
  provider,
  appType,
  onClose,
  onSave,
  onNotify,
}) => {
  const [script, setScript] = useState<UsageScript>(() => {
    return (
      provider.meta?.usage_script || {
        enabled: false,
        language: "javascript",
        code: PRESET_TEMPLATES["通用模板"],
        timeout: 10,
      }
    );
  });

  const [testing, setTesting] = useState(false);

  const handleSave = () => {
    // 验证脚本格式
    if (script.enabled && !script.code.trim()) {
      onNotify?.("脚本配置不能为空", "error");
      return;
    }

    // 基本的 JS 语法检查（检查是否包含 return 语句）
    if (script.enabled && !script.code.includes("return")) {
      onNotify?.("脚本必须包含 return 语句", "error", 5000);
      return;
    }

    onSave(script);
    onClose();
    onNotify?.("用量查询配置已保存", "success", 2000);
  };

  const handleTest = async () => {
    setTesting(true);
    try {
      const result = await window.api.queryProviderUsage(
        provider.id,
        appType
      );
      if (result.success && result.data && result.data.length > 0) {
        // 显示所有套餐数据
        const summary = result.data
          .map((plan) => {
            const planInfo = plan.planName ? `[${plan.planName}]` : "";
            return `${planInfo} 剩余: ${plan.remaining} ${plan.unit}`;
          })
          .join(", ");
        onNotify?.(`测试成功！${summary}`, "success", 3000);
      } else {
        onNotify?.(`测试失败: ${result.error || "无数据返回"}`, "error", 5000);
      }
    } catch (error: any) {
      onNotify?.(`测试失败: ${error?.message || "未知错误"}`, "error", 5000);
    } finally {
      setTesting(false);
    }
  };

  const handleFormat = async () => {
    try {
      const formatted = await prettier.format(script.code, {
        parser: "babel",
        plugins: [parserBabel as any, pluginEstree as any],
        semi: true,
        singleQuote: false,
        tabWidth: 2,
        printWidth: 80,
      });
      setScript({ ...script, code: formatted.trim() });
      onNotify?.("格式化成功", "success", 1000);
    } catch (error: any) {
      onNotify?.(`格式化失败: ${error?.message || "语法错误"}`, "error", 3000);
    }
  };

  const handleUsePreset = (presetName: string) => {
    const preset = PRESET_TEMPLATES[presetName];
    if (preset) {
      setScript({ ...script, code: preset });
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-white dark:bg-gray-900 rounded-lg shadow-xl w-full max-w-3xl max-h-[90vh] overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-xl font-semibold text-gray-900 dark:text-gray-100">
            配置用量查询 - {provider.name}
          </h2>
          <button
            onClick={onClose}
            className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* 启用开关 */}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={script.enabled}
              onChange={(e) =>
                setScript({ ...script, enabled: e.target.checked })
              }
              className="w-4 h-4"
            />
            <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
              启用用量查询
            </span>
          </label>

          {script.enabled && (
            <>
              {/* 预设模板选择 */}
              <div>
                <label className="block text-sm font-medium mb-2 text-gray-900 dark:text-gray-100">
                  预设模板
                </label>
                <div className="flex gap-2">
                  {Object.keys(PRESET_TEMPLATES).map((name) => (
                    <button
                      key={name}
                      onClick={() => handleUsePreset(name)}
                      className="px-3 py-1.5 text-xs bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 rounded transition-colors"
                    >
                      {name}
                    </button>
                  ))}
                </div>
              </div>

              {/* 脚本编辑器 */}
              <div>
                <label className="block text-sm font-medium mb-2 text-gray-900 dark:text-gray-100">
                  查询脚本（JavaScript）
                </label>
                <JsonEditor
                  value={script.code}
                  onChange={(code) => setScript({ ...script, code })}
                  height="300px"
                  language="javascript"
                />
                <p className="mt-2 text-xs text-gray-500 dark:text-gray-400">
                  支持变量: <code>{"{{apiKey}}"}</code>,{" "}
                  <code>{"{{baseUrl}}"}</code> | extractor 函数接收 API 响应的 JSON 对象
                </p>
              </div>

              {/* 配置选项 */}
              <div className="grid grid-cols-2 gap-4">
                <label className="block">
                  <span className="text-sm font-medium text-gray-900 dark:text-gray-100">
                    超时时间（秒）
                  </span>
                  <input
                    type="number"
                    min="2"
                    max="30"
                    value={script.timeout || 10}
                    onChange={(e) =>
                      setScript({ ...script, timeout: parseInt(e.target.value) })
                    }
                    className="mt-1 w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100"
                  />
                </label>
              </div>

              {/* 脚本说明 */}
              <div className="p-4 bg-blue-50 dark:bg-blue-900/20 rounded-lg text-sm text-gray-700 dark:text-gray-300">
                <h4 className="font-medium mb-2">脚本编写说明：</h4>
                <div className="space-y-3 text-xs">
                  <div>
                    <strong>配置格式：</strong>
                    <pre className="mt-1 p-2 bg-white/50 dark:bg-black/20 rounded text-[10px] overflow-x-auto">
{`({
  request: {
    url: "{{baseUrl}}/api/usage",
    method: "POST",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "User-Agent": "cc-switch/1.0"
    },
    body: JSON.stringify({ key: "value" })  // 可选
  },
  extractor: function(response) {
    // response 是 API 返回的 JSON 数据
    return {
      isValid: !response.error,
      remaining: response.balance,
      unit: "USD"
    };
  }
})`}
                    </pre>
                  </div>

                  <div>
                    <strong>extractor 返回格式（所有字段均为可选）：</strong>
                    <ul className="mt-1 space-y-0.5 ml-2">
                      <li>• <code>isValid</code>: 布尔值，套餐是否有效</li>
                      <li>• <code>invalidMessage</code>: 字符串，失效原因说明（当 isValid 为 false 时显示）</li>
                      <li>• <code>remaining</code>: 数字，剩余额度</li>
                      <li>• <code>unit</code>: 字符串，单位（如 "USD"）</li>
                      <li>• <code>planName</code>: 字符串，套餐名称</li>
                      <li>• <code>total</code>: 数字，总额度</li>
                      <li>• <code>used</code>: 数字，已用额度</li>
                      <li>• <code>extra</code>: 字符串，扩展字段，可自由补充需要展示的文本</li>
                    </ul>
                  </div>

                  <div className="text-gray-600 dark:text-gray-400">
                    <strong>💡 提示：</strong>
                    <ul className="mt-1 space-y-0.5 ml-2">
                      <li>• 变量 <code>{"{{apiKey}}"}</code> 和 <code>{"{{baseUrl}}"}</code> 会自动替换</li>
                      <li>• extractor 函数在沙箱环境中执行，支持 ES2020+ 语法</li>
                      <li>• 整个配置必须用 <code>()</code> 包裹，形成对象字面量表达式</li>
                    </ul>
                  </div>
                </div>
              </div>
            </>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-gray-200 dark:border-gray-700">
          <div className="flex gap-2">
            <button
              onClick={handleTest}
              disabled={!script.enabled || testing}
              className="inline-flex items-center gap-2 px-4 py-2 bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Play size={14} />
              {testing ? "测试中..." : "测试脚本"}
            </button>
            <button
              onClick={handleFormat}
              disabled={!script.enabled}
              className="inline-flex items-center gap-2 px-4 py-2 bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              title="格式化代码 (Prettier)"
            >
              <Wand2 size={14} />
              格式化
            </button>
          </div>

          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-md hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors"
            >
              取消
            </button>
            <button
              onClick={handleSave}
              className="px-4 py-2 bg-blue-500 text-white rounded-md hover:bg-blue-600 transition-colors"
            >
              保存配置
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};

export default UsageScriptModal;
