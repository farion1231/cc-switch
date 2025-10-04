import React, { useCallback, useEffect, useMemo, useState } from "react";
import { Zap, Loader2, Plus, Trash2, AlertCircle, Check } from "lucide-react";

import type { AppType } from "../../lib/tauri-api";

export interface EndpointCandidate {
  id?: string;
  url: string;
  label?: string;
  isCustom?: boolean;
}

interface EndpointSpeedTestProps {
  appType: AppType;
  value: string;
  onChange: (url: string) => void;
  initialEndpoints: EndpointCandidate[];
  visible?: boolean;
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
    if (map.has(sanitized)) {
      const existing = map.get(sanitized)!;
      if (candidate.label && candidate.label !== existing.label) {
        map.set(sanitized, { ...existing, label: candidate.label });
      }
      return;
    }
    const index = map.size;
    const label =
      candidate.label ??
      (candidate.isCustom
        ? `自定义 ${index + 1}`
        : index === 0
          ? "默认地址"
          : `候选 ${index + 1}`);
    map.set(sanitized, {
      id: candidate.id ?? randomId(),
      url: sanitized,
      label,
      isCustom: candidate.isCustom ?? false,
      latency: null,
      status: undefined,
      error: null,
    });
  };

  candidates.forEach(addCandidate);

  const selectedUrl = normalizeEndpointUrl(selected);
  if (selectedUrl && !map.has(selectedUrl)) {
    addCandidate({ url: selectedUrl, label: "当前地址", isCustom: true });
  }

  return Array.from(map.values());
};

const EndpointSpeedTest: React.FC<EndpointSpeedTestProps> = ({
  appType,
  value,
  onChange,
  initialEndpoints,
  visible = true,
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
        if (existing) {
          if (candidate.label && candidate.label !== existing.label) {
            map.set(sanitized, { ...existing, label: candidate.label });
            changed = true;
          }
          return;
        }
        const index = map.size;
        const label =
          candidate.label ??
          (candidate.isCustom
            ? `自定义 ${index + 1}`
            : index === 0
              ? "默认地址"
              : `候选 ${index + 1}`);
        map.set(sanitized, {
          id: candidate.id ?? randomId(),
          url: sanitized,
          label,
          isCustom: candidate.isCustom ?? false,
          latency: null,
          status: undefined,
          error: null,
        });
        changed = true;
      };

      initialEndpoints.forEach(mergeCandidate);

      if (normalizedSelected) {
        const existing = map.get(normalizedSelected);
        if (existing) {
          if (existing.label !== "当前地址") {
            map.set(normalizedSelected, {
              ...existing,
              label: existing.isCustom ? existing.label : "当前地址",
            });
            changed = true;
          }
        } else {
          mergeCandidate({ url: normalizedSelected, label: "当前地址", isCustom: true });
        }
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
      setAddError("URL 格式不正确，请确认包含 http(s) 前缀");
      return;
    }

    if (!parsed.protocol.startsWith("http")) {
      setAddError("仅支持 HTTP/HTTPS 地址");
      return;
    }

    const sanitized = normalizeEndpointUrl(parsed.toString());

    setEntries((prev) => {
      if (prev.some((entry) => entry.url === sanitized)) {
        setAddError("该地址已存在");
        return prev;
      }
      const customCount = prev.filter((entry) => entry.isCustom).length;
      return [
        ...prev,
        {
          id: randomId(),
          url: sanitized,
          label: `自定义 ${customCount + 1}`,
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
      setLastError("请先添加至少一个地址再进行测速");
      return;
    }

    if (typeof window === "undefined" || !window.api?.testApiEndpoints) {
      setLastError("测速功能仅在桌面应用中可用");
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
              error: "未返回测速结果",
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

  if (!visible) {
    return null;
  }

  return (
    <div className="mt-4 space-y-3 rounded-lg border border-gray-200 bg-gray-50 p-4 dark:border-gray-700 dark:bg-gray-900/40">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-gray-800 dark:text-gray-200">
            节点测速
          </h3>
          <p className="text-xs text-gray-500 dark:text-gray-400">
            添加多个端点后可一键测速，自动选取延迟最低的地址
          </p>
        </div>
        <div className="flex items-center gap-3">
          <label className="flex items-center gap-2 text-xs text-gray-600 dark:text-gray-400">
            <input
              type="checkbox"
              checked={autoSelect}
              onChange={(event) => setAutoSelect(event.target.checked)}
              className="rounded border-gray-300 text-blue-500 focus:ring-blue-400"
            />
            自动选择最快节点
          </label>
          <button
            type="button"
            onClick={runSpeedTest}
            disabled={isTesting || entries.length === 0}
            className="flex items-center gap-2 rounded-md bg-blue-500 px-3 py-1.5 text-sm text-white transition hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {isTesting ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                测速中...
              </>
            ) : (
              <>
                <Zap className="h-4 w-4" />
                开始测速
              </>
            )}
          </button>
        </div>
      </div>

      {hasEndpoints ? (
        <div className="space-y-2">
          {sortedEntries.map((entry) => {
            const isSelected = normalizedSelected === entry.url;
            const latency = entry.latency;
            const statusBadge =
              latency !== null
                ? latency <= 100
                  ? "text-green-600 dark:text-green-400"
                  : latency <= 300
                    ? "text-amber-600 dark:text-amber-400"
                    : "text-red-600 dark:text-red-400"
                : "text-gray-500 dark:text-gray-400";

            return (
              <div
                key={entry.id}
                className={`flex items-start justify-between gap-2 rounded-lg border px-3 py-2 text-sm transition ${
                  isSelected
                    ? "border-green-400 bg-green-50 dark:border-green-500 dark:bg-green-900/30"
                    : "border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800"
                }`}
              >
                <label className="flex flex-1 cursor-pointer items-start gap-2">
                  <input
                    type="radio"
                    name="endpoint-speedtest"
                    checked={isSelected}
                    onChange={() => handleSelect(entry.url)}
                    className="mt-1 h-4 w-4 border-gray-300 text-green-500 focus:ring-green-400"
                  />
                  <div className="flex flex-1 flex-col gap-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-gray-800 dark:text-gray-100">
                        {entry.label || "候选节点"}
                      </span>
                      {isSelected && (
                        <span className="flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                          <Check className="h-3 w-3" /> 已选中
                        </span>
                      )}
                    </div>
                    <span className="break-all text-xs text-gray-500 dark:text-gray-400">
                      {entry.url}
                    </span>
                  </div>
                </label>
                <div className="flex items-center gap-3">
                  <div className="text-xs font-mono">
                    {latency !== null ? (
                      <span className={statusBadge}>{latency} ms</span>
                    ) : isTesting ? (
                      <span className="text-gray-400">等待结果</span>
                    ) : entry.error ? (
                      <span className="flex items-center gap-1 text-red-500">
                        <AlertCircle className="h-3 w-3" />
                        失败
                      </span>
                    ) : (
                      <span className="text-gray-400">未测速</span>
                    )}
                  </div>
                  {entry.isCustom && (
                    <button
                      type="button"
                      onClick={() => handleRemoveEndpoint(entry)}
                      className="rounded-md p-1 text-red-500 transition hover:bg-red-50 hover:text-red-600 dark:hover:bg-red-900/30"
                      title="删除该地址"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      ) : (
        <div className="rounded-md border border-dashed border-gray-300 bg-white p-4 text-center text-xs text-gray-500 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-400">
          暂无可测速的地址，请先添加至少一个请求地址
        </div>
      )}

      <div>
        <div className="flex gap-2">
          <input
            type="url"
            value={customUrl}
            placeholder="https://example.com/claude"
            onChange={(event) => setCustomUrl(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                handleAddEndpoint();
              }
            }}
            className="flex-1 rounded-md border border-gray-200 px-3 py-2 text-sm text-gray-800 shadow-sm transition focus:border-blue-500 focus:outline-none focus:ring-2 focus:ring-blue-400/20 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100"
          />
          <button
            type="button"
            onClick={handleAddEndpoint}
            className="flex items-center gap-1 rounded-md bg-green-500 px-3 py-1.5 text-sm text-white transition hover:bg-green-600"
          >
            <Plus className="h-4 w-4" />
            添加
          </button>
        </div>
        {addError && (
          <p className="mt-1 text-xs text-red-500">{addError}</p>
        )}
      </div>

      {lastError && (
        <p className="text-xs text-red-500">
          <AlertCircle className="mr-1 inline h-3 w-3 align-middle" />
          {lastError}
        </p>
      )}
    </div>
  );
};

export default EndpointSpeedTest;
