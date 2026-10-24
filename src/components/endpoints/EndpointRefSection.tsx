import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable } from "lucide-react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { endpointsApi } from "@/lib/api";
import { EndpointFormModal } from "./EndpointFormModal";
import type { EndpointsMap } from "@/types";

const STANDALONE = "__standalone__";

interface EndpointRefSectionProps {
  endpointRef?: string;
  onChange: (endpointRef: string | undefined) => void;
}

/**
 * 条目的连接端点引用。
 *
 * 引用后条目的地址/密钥由全局端点提供（保存与端点更新时自动物化，
 * 表单里的地址/密钥字段仅展示当前物化结果）。档位级跨端点选择在
 * 「模型映射」行内进行（tierEndpoints 传入 ClaudeFormFields）。
 */
export function EndpointRefSection({
  endpointRef,
  onChange,
}: EndpointRefSectionProps) {
  const { t } = useTranslation();
  const [endpoints, setEndpoints] = useState<EndpointsMap>({});
  const [createOpen, setCreateOpen] = useState(false);

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

  return (
    <div className="space-y-3 rounded-xl border p-4">
      <div className="flex items-center gap-2">
        <Cable className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-semibold">
          {t("providerForm.endpointSectionTitle", {
            defaultValue: "连接端点",
          })}
        </h3>
      </div>

      <div className="space-y-1.5">
        <Label>
          {t("providerForm.endpointRef", { defaultValue: "连接端点" })}
        </Label>
        <Select
          value={endpointRef ?? STANDALONE}
          onValueChange={(value) => {
            if (value === "__create__") {
              setCreateOpen(true);
              return;
            }
            onChange(value === STANDALONE ? undefined : value);
          }}
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
            <SelectItem value="__create__">
              + {t("endpoints.add", { defaultValue: "新建端点" })}
            </SelectItem>
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

      <EndpointFormModal
        isOpen={createOpen}
        onClose={() => setCreateOpen(false)}
        editing={null}
        onSave={async (endpoint) => {
          await endpointsApi.save(endpoint);
          setEndpoints((prev) => ({ ...prev, [endpoint.id]: endpoint }));
          onChange(endpoint.id);
          setCreateOpen(false);
        }}
      />
    </div>
  );
}
