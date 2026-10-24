import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable, X } from "lucide-react";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { endpointsApi } from "@/lib/api";
import type { EndpointsMap, TierMappings, TierMapping } from "@/types";

type TierKey = keyof TierMappings;

const TIER_ORDER: TierKey[] = [
  "sonnet",
  "opus",
  "fable",
  "haiku",
  "subagent",
  "default",
];

const STANDALONE = "__standalone__";

interface EndpointRefSectionProps {
  endpointRef?: string;
  tierMappings?: TierMappings;
  onChange: (next: {
    endpointRef?: string;
    tierMappings?: TierMappings;
  }) => void;
}

/**
 * 条目的端点引用与档位映射区块。
 *
 * 端点引用：条目的地址/密钥改由全局端点提供（保存与端点更新时自动物化，
 * 表单里的地址/密钥字段仅展示当前物化结果）。
 * 档位映射：某档"仅改模型名"（本端点内换档），或"跨端点分流"（代理模式下
 * 该档请求发往另一端点，如标题生成走便宜家；直连不生效）。
 */
export function EndpointRefSection({
  endpointRef,
  tierMappings,
  onChange,
}: EndpointRefSectionProps) {
  const { t } = useTranslation();
  const [endpoints, setEndpoints] = useState<EndpointsMap>({});

  useEffect(() => {
    let cancelled = false;
    endpointsApi
      .getAll()
      .then((map) => {
        if (!cancelled) setEndpoints(map);
      })
      .catch(() => {
        // 端点列表加载失败不阻塞编辑：下拉为空，引用保持原值
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const endpointList = useMemo(() => Object.values(endpoints), [endpoints]);
  const selected = endpointRef ? endpoints[endpointRef] : undefined;

  const setTier = (key: TierKey, next: TierMapping | null) => {
    const mappings: TierMappings = { ...tierMappings };
    if (next) {
      mappings[key] = next;
    } else {
      delete mappings[key];
    }
    // 全空则整体置空，避免保存空对象
    const hasAny = TIER_ORDER.some((k) => mappings[k]);
    onChange({
      endpointRef,
      tierMappings: hasAny ? mappings : undefined,
    });
  };

  return (
    <div className="space-y-4 rounded-xl border p-4">
      <div className="flex items-center gap-2">
        <Cable className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-semibold">
          {t("providerForm.endpointSectionTitle", {
            defaultValue: "端点引用与档位映射",
          })}
        </h3>
      </div>

      <div className="space-y-1.5">
        <Label>
          {t("providerForm.endpointRef", { defaultValue: "连接端点" })}
        </Label>
        <Select
          value={endpointRef ?? STANDALONE}
          onValueChange={(value) =>
            onChange({
              endpointRef: value === STANDALONE ? undefined : value,
              tierMappings,
            })
          }
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={STANDALONE}>
              {t("providerForm.endpointStandalone", {
                defaultValue: "独立配置（地址密钥在本条目内填写）",
              })}
            </SelectItem>
            {endpointList.map((endpoint) => (
              <SelectItem key={endpoint.id} value={endpoint.id}>
                {endpoint.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-fg-3">
          {selected
            ? t("providerForm.endpointRefActive", {
                defaultValue: `已引用「${selected.name}」：地址与密钥以端点为准（在端点管理里改，本条目保存时自动更新）`,
                name: selected.name,
              })
            : t("providerForm.endpointRefHint", {
                defaultValue:
                  "引用全局端点后，同一家厂商的多个条目共享一份密钥，改一处全部生效",
              })}
        </p>
      </div>

      <div className="space-y-2">
        <Label>
          {t("providerForm.tierMappings", { defaultValue: "档位映射" })}
        </Label>
        <p className="text-xs text-fg-3">
          {t("providerForm.tierMappingsHint", {
            defaultValue:
              "填模型名=该档在本端点内换模型；选「跨端点」=代理模式下该档请求发往其他端点（如 Haiku 档走便宜中转），直连模式不生效",
          })}
        </p>
        {TIER_ORDER.map((key) => {
          const tier = tierMappings?.[key];
          const crossEndpoint = tier?.endpoint;
          return (
            <div key={key} className="flex items-center gap-2">
              <span className="w-16 shrink-0 text-xs capitalize text-fg-2">
                {key}
              </span>
              <Input
                value={tier?.model ?? ""}
                placeholder={t("providerForm.tierModelPlaceholder", {
                  defaultValue: "模型名（留空=不映射）",
                })}
                onChange={(e) =>
                  setTier(key, {
                    endpoint: crossEndpoint,
                    model: e.target.value.trim() || undefined,
                  })
                }
                className="h-8 flex-1"
              />
              <Select
                value={crossEndpoint ?? "__self__"}
                onValueChange={(value) =>
                  setTier(key, {
                    endpoint:
                      value === "__self__" ? undefined : value || undefined,
                    model: tier?.model,
                  })
                }
              >
                <SelectTrigger className="h-8 w-44 shrink-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__self__">
                    {t("providerForm.tierSelfEndpoint", {
                      defaultValue: "本端点",
                    })}
                  </SelectItem>
                  {endpointList.map((endpoint) => (
                    <SelectItem key={endpoint.id} value={endpoint.id}>
                      {t("providerForm.tierCrossEndpoint", {
                        defaultValue: "跨端点 → {{name}}",
                        name: endpoint.name,
                      })}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {tier && (
                <button
                  type="button"
                  className="shrink-0 text-fg-3 hover:text-fg-1"
                  onClick={() => setTier(key, null)}
                  title={t("providerForm.tierClear", {
                    defaultValue: "清除该档映射",
                  })}
                >
                  <X className="h-4 w-4" />
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
