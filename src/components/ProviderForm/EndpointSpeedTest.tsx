import React, { useCallback, useEffect, useMemo, useState } from "react";
import { Zap, Loader2, Plus, X, AlertCircle } from "lucide-react";
import { isLinux } from "../../lib/platform";

import type { AppType } from "../../lib/tauri-api";

export interface EndpointCandidate {
  id?: string;
  url: string;
  isCustom?: boolean;
}

interface EndpointSpeedTestProps {
  appType: AppType;
  value: string;
  onChange: (url: string) => void;
  initialEndpoints: EndpointCandidate[];
  visible?: boolean;
  onClose: () => void;
}

interface EndpointEntry extends EndpointCandidate {
  id: string;
  latency: number | null;
  status?: number;
  error?: string | null;
}

const randomId = () => `ep_${Math.random().toString(36).slice(2, 9)}`;

const normalizeEndpointUrl = (url: string): string =>
  url.trim().replace(/\/+$/, "");

const buildInitialEntries = (
  candidates: EndpointCandidate[],
  selected: string,
): EndpointEntry[] => {
  const map = new Map<string, EndpointEntry>();
  const addCandidate = (candidate: EndpointCandidate) => {
    const sanitized = candidate.url ? normalizeEndpointUrl(candidate.url) : "";
    if (!sanitized) return;
    if (map.has(sanitized)) return;

    map.set(sanitized, {
      id: candidate.id ?? randomId(),
      url: sanitized,
      isCustom: candidate.isCustom ?? false,
      latency: null,
      status: undefined,
      error: null,
    });
  };

  candidates.forEach(addCandidate);

  const selectedUrl = normalizeEndpointUrl(selected);
  if (selectedUrl && !map.has(selectedUrl)) {
    addCandidate({ url: selectedUrl, isCustom: true });
  }

  return Array.from(map.values());
};

const EndpointSpeedTest: React.FC<EndpointSpeedTestProps> = ({
  appType,
  value,
  onChange,
  initialEndpoints,
  visible = true,
  onClose,
}) => {
  const [entries, setEntries] = useState<EndpointEntry[]>(() =>
    buildInitialEntries(initialEndpoints, value),
  );
  const [customUrl, setCustomUrl] = useState("");
  const [addError, setAddError] = useState<string | null>(null);
  const [autoSelect, setAutoSelect] = useState(true);
  const [isTesting, setIsTesting] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const normalizedSelected = normalizeEndpointUrl(value);

  const hasEndpoints = entries.length > 0;

  useEffect(() => {
    setEntries((prev) => {
      const map = new Map<string, EndpointEntry>();
      prev.forEach((entry) => {
        map.set(entry.url, entry);
      });

      let changed = false;

      const mergeCandidate = (candidate: EndpointCandidate) => {
        const sanitized = candidate.url
          ? normalizeEndpointUrl(candidate.url)
          : "";
        if (!sanitized) return;
        const existing = map.get(sanitized);
        if (existing) return;

        map.set(sanitized, {
          id: candidate.id ?? randomId(),
          url: sanitized,
          isCustom: candidate.isCustom ?? false,
          latency: null,
          status: undefined,
          error: null,
        });
        changed = true;
      };

      initialEndpoints.forEach(mergeCandidate);

      if (normalizedSelected && !map.has(normalizedSelected)) {
        mergeCandidate({ url: normalizedSelected, isCustom: true });
      }

      if (!changed) {
        return prev;
      }

      return Array.from(map.values());
    });
  }, [initialEndpoints, normalizedSelected]);

  const sortedEntries = useMemo(() => {
    return entries.slice().sort((a, b) => {
      const aLatency = a.latency ?? Number.POSITIVE_INFINITY;
      const bLatency = b.latency ?? Number.POSITIVE_INFINITY;
      if (aLatency === bLatency) {
        return a.url.localeCompare(b.url);
      }
      return aLatency - bLatency;
    });
  }, [entries]);

  const handleAddEndpoint = useCallback(() => {
    const candidate = customUrl.trim();
    setAddError(null);

    if (!candidate) {
      setAddError("请输入有效的 URL");
      return;
    }

    let parsed: URL;
    try {
      parsed = new URL(candidate);
    } catch {
      setAddError("URL 格式不正确");
      return;
    }

    if (!parsed.protocol.startsWith("http")) {
      setAddError("仅支持 HTTP/HTTPS");
      return;
    }

    const sanitized = normalizeEndpointUrl(parsed.toString());

    setEntries((prev) => {
      if (prev.some((entry) => entry.url === sanitized)) {
        setAddError("该地址已存在");
        return prev;
      }
      return [
        ...prev,
        {
          id: randomId(),
          url: sanitized,
          isCustom: true,
          latency: null,
          status: undefined,
          error: null,
        },
      ];
    });

    if (!normalizedSelected) {
      onChange(sanitized);
    }

    setCustomUrl("");
  }, [customUrl, normalizedSelected, onChange]);

  const handleRemoveEndpoint = useCallback(
    (entry: EndpointEntry) => {
      setEntries((prev) => {
        const next = prev.filter((item) => item.id !== entry.id);
        if (entry.url === normalizedSelected) {
          const fallback = next[0];
          onChange(fallback ? fallback.url : "");
        }
        return next;
      });
    },
    [normalizedSelected, onChange],
  );

  const runSpeedTest = useCallback(async () => {
    const urls = entries.map((entry) => entry.url);
    if (urls.length === 0) {
      setLastError("请先添加端点");
      return;
    }

    if (typeof window === "undefined" || !window.api?.testApiEndpoints) {
      setLastError("测速功能不可用");
      return;
    }

    setIsTesting(true);
    setLastError(null);

    try {
      const results = await window.api.testApiEndpoints(urls, {
        timeoutSecs: appType === "codex" ? 12 : 8,
      });
      const resultMap = new Map(
        results.map((item) => [normalizeEndpointUrl(item.url), item]),
      );

      setEntries((prev) =>
        prev.map((entry) => {
          const match = resultMap.get(entry.url);
          if (!match) {
            return {
              ...entry,
              latency: null,
              status: undefined,
              error: "未返回结果",
            };
          }
          return {
            ...entry,
            latency:
              typeof match.latency === "number" ? Math.round(match.latency) : null,
            status: match.status,
            error: match.error ?? null,
          };
        }),
      );

      if (autoSelect) {
        const successful = results
          .filter((item) => typeof item.latency === "number" && item.latency !== null)
          .sort((a, b) => (a.latency! || 0) - (b.latency! || 0));
        const best = successful[0];
        if (best && best.url && best.url !== normalizedSelected) {
          onChange(best.url);
        }
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : `测速失败: ${String(error)}`;
      setLastError(message);
    } finally {
      setIsTesting(false);
    }
  }, [entries, autoSelect, appType, normalizedSelected, onChange]);

  const handleSelect = useCallback(
    (url: string) => {
      if (!url || url === normalizedSelected) return;
      onChange(url);
    },
    [normalizedSelected, onChange],
  );

  // 支持按下 ESC 关闭弹窗
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  if (!visible) {
    return null;
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      {/* Backdrop */}
      <div
        className={`absolute inset-0 bg-black/50 dark:bg-black/70${
          isLinux() ? "" : " backdrop-blur-sm"
        }`}
      />

      {/* Modal */}
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-lg w-full max-w-2xl mx-4 max-h-[80vh] overflow-hidden flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-800">
          <h3 className="text-base font-medium text-gray-900 dark:text-gray-100">
            请求地址管理
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="p-1 text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
            aria-label="关闭"
          >
            <X size={16} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-auto px-6 py-4 space-y-4">
          {/* 测速控制栏 */}
          <div className="flex items-center justify-between">
            <div className="text-sm text-gray-600 dark:text-gray-400">
              {entries.length} 个端点
            </div>
            <div className="flex items-center gap-3">
              <label className="flex items-center gap-1.5 text-xs text-gray-600 dark:text-gray-400">
                <input
                  type="checkbox"
                  checked={autoSelect}
                  onChange={(event) => setAutoSelect(event.target.checked)}
                  className="h-3.5 w-3.5 rounded border-gray-300 dark:border-gray-600"
                />
                自动选择
              </label>
              <button
                type="button"
                onClick={runSpeedTest}
                disabled={isTesting || !hasEndpoints}
                className="flex h-7 items-center gap-1.5 rounded-md bg-blue-500 px-2.5 text-xs font-medium text-white transition hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-40 dark:bg-blue-600 dark:hover:bg-blue-700"
              >
                {isTesting ? (
                  <>
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    测速中
                  </>
                ) : (
                  <>
                    <Zap className="h-3.5 w-3.5" />
                    测速
                  </>
                )}
              </button>
            </div>
          </div>

          {/* 添加输入 */}
          <div className="space-y-1.5">
            <div className="flex gap-2">
              <input
                type="url"
                value={customUrl}
                placeholder="https://api.example.com"
                onChange={(event) => setCustomUrl(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    handleAddEndpoint();
                  }
                }}
                className="flex-1 rounded-md border border-gray-200 bg-white px-3 py-1.5 text-sm text-gray-900 placeholder-gray-400 transition focus:border-gray-400 focus:outline-none dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100 dark:placeholder-gray-500 dark:focus:border-gray-600"
              />
              <button
                type="button"
                onClick={handleAddEndpoint}
                className="flex h-8 w-8 items-center justify-center rounded-md border border-gray-200 transition hover:border-gray-300 hover:bg-gray-50 dark:border-gray-700 dark:hover:border-gray-600 dark:hover:bg-gray-800"
              >
                <Plus className="h-4 w-4 text-gray-600 dark:text-gray-400" />
              </button>
            </div>
            {addError && (
              <div className="flex items-center gap-1.5 text-xs text-red-600 dark:text-red-400">
                <AlertCircle className="h-3 w-3" />
                {addError}
              </div>
            )}
          </div>

          {/* 端点列表 */}
          {hasEndpoints ? (
            <div className="space-y-px overflow-hidden rounded-md border border-gray-200 dark:border-gray-700">
              {sortedEntries.map((entry, index) => {
                const isSelected = normalizedSelected === entry.url;
                const latency = entry.latency;

                return (
                  <div
                    key={entry.id}
                    onClick={() => handleSelect(entry.url)}
                    className={`group flex cursor-pointer items-center justify-between px-3 py-2.5 transition ${
                      isSelected
                        ? "bg-gray-100 dark:bg-gray-800"
                        : "bg-white hover:bg-gray-50 dark:bg-gray-900 dark:hover:bg-gray-850"
                    } ${index > 0 ? "border-t border-gray-100 dark:border-gray-800" : ""}`}
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-3">
                      {/* 选择指示器 */}
                      <div
                        className={`h-1.5 w-1.5 flex-shrink-0 rounded-full transition ${
                          isSelected
                            ? "bg-gray-900 dark:bg-gray-100"
                            : "bg-gray-300 dark:bg-gray-700"
                        }`}
                      />

                      {/* 内容 */}
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm text-gray-900 dark:text-gray-100">
                          {entry.url}
                        </div>
                      </div>
                    </div>

                    {/* 右侧信息 */}
                    <div className="flex items-center gap-2">
                      {latency !== null ? (
                        <div className="text-right">
                          <div className="font-mono text-sm font-medium text-gray-900 dark:text-gray-100">
                            {latency}ms
                          </div>
                        </div>
                      ) : isTesting ? (
                        <Loader2 className="h-4 w-4 animate-spin text-gray-400" />
                      ) : entry.error ? (
                        <div className="text-xs text-gray-400">失败</div>
                      ) : (
                        <div className="text-xs text-gray-400">—</div>
                      )}

                      {entry.isCustom && (
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleRemoveEndpoint(entry);
                          }}
                          className="opacity-0 transition hover:text-red-600 group-hover:opacity-100 dark:hover:text-red-400"
                        >
                          <X className="h-4 w-4" />
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="rounded-md border border-dashed border-gray-200 bg-gray-50 py-8 text-center text-xs text-gray-500 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-400">
              暂无端点
            </div>
          )}

          {/* 错误提示 */}
          {lastError && (
            <div className="flex items-center gap-1.5 text-xs text-red-600 dark:text-red-400">
              <AlertCircle className="h-3 w-3" />
              {lastError}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-3 p-6 border-t border-gray-200 dark:border-gray-800 bg-gray-100 dark:bg-gray-800">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 bg-blue-500 dark:bg-blue-600 text-white rounded-lg hover:bg-blue-600 dark:hover:bg-blue-700 transition-colors text-sm font-medium"
          >
            完成
          </button>
        </div>
      </div>
    </div>
  );
};

export default EndpointSpeedTest;
