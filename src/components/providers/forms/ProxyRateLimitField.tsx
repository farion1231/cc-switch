import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import type { ProxyRateLimitConfig } from "@/types";

interface ProxyRateLimitFieldProps {
  value: ProxyRateLimitConfig | undefined;
  onChange: (value: ProxyRateLimitConfig) => void;
}

const DEFAULT_RPM = 18;
const DEFAULT_BURST = 2;
const DEFAULT_MAX_WAIT_MS = 30000;

function toOptionalNumber(raw: string): number | undefined {
  if (raw.trim() === "") return undefined;
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : undefined;
}

/**
 * 供应商级请求削峰（客户端限流）字段。
 *
 * 默认关闭；开启后在本地代理内按令牌桶排队，超过每分钟配额的请求先等待再发往
 * 上游。排队超时即放行，因此这项配置只会增加延迟，不会让请求失败。
 */
export function ProxyRateLimitField({
  value,
  onChange,
}: ProxyRateLimitFieldProps) {
  const { t } = useTranslation();
  const enabled = value?.enabled ?? false;
  const label = t("providerForm.proxyRateLimit", {
    defaultValue: "请求削峰（限流）",
  });

  const patch = (next: Partial<ProxyRateLimitConfig>) => {
    onChange({ enabled, ...value, ...next });
  };

  return (
    <div className="space-y-3">
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-center gap-0.5">
          <FormLabel>{label}</FormLabel>
          <HelpTip title={label}>
            {t("providerForm.proxyRateLimitHelp", {
              defaultValue:
                "在本地代理内按令牌桶排队：超过每分钟配额的请求先等待再发往上游，避免供应商按分钟限流直接返回 429。只对经过本地路由的请求生效。",
            })}
          </HelpTip>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={(checked) => patch({ enabled: checked })}
          aria-label={t("providerForm.proxyRateLimitEnable", {
            defaultValue: "启用请求削峰",
          })}
        />
      </div>

      {!enabled ? (
        <p className="text-xs leading-relaxed text-fg-2">
          {t("providerForm.proxyRateLimitOffHint", {
            defaultValue:
              "默认关闭。开启后请求会先排队再发出，排队超时即放行，不会因此失败。",
          })}
        </p>
      ) : (
        <div className="grid gap-3 md:grid-cols-3">
          <div className="space-y-2">
            <FormLabel className="text-xs text-fg-2">
              {t("providerForm.proxyRateLimitRpm", {
                defaultValue: "每分钟请求上限",
              })}
            </FormLabel>
            <Input
              type="number"
              min={1}
              max={600}
              step={1}
              value={value?.rpm ?? ""}
              placeholder={String(DEFAULT_RPM)}
              onChange={(event) =>
                patch({ rpm: toOptionalNumber(event.target.value) })
              }
            />
            <p className="text-xs leading-relaxed text-fg-2">
              {t("providerForm.proxyRateLimitRpmHint", {
                defaultValue:
                  "按上游套餐的实际配额填写，例如火山 Coding Plan 实测约 20 次/分钟。",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <FormLabel className="text-xs text-fg-2">
              {t("providerForm.proxyRateLimitBurst", {
                defaultValue: "突发容量",
              })}
            </FormLabel>
            <Input
              type="number"
              min={1}
              max={50}
              step={1}
              value={value?.burst ?? ""}
              placeholder={String(DEFAULT_BURST)}
              onChange={(event) =>
                patch({ burst: toOptionalNumber(event.target.value) })
              }
            />
            <p className="text-xs leading-relaxed text-fg-2">
              {t("providerForm.proxyRateLimitBurstHint", {
                defaultValue:
                  "允许瞬间连续放行的请求数，之后按上面的速率逐个补充。",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <FormLabel className="text-xs text-fg-2">
              {t("providerForm.proxyRateLimitMaxWait", {
                defaultValue: "最长排队 (毫秒)",
              })}
            </FormLabel>
            <Input
              type="number"
              min={0}
              max={300000}
              step={1000}
              value={value?.maxWaitMs ?? ""}
              placeholder={String(DEFAULT_MAX_WAIT_MS)}
              onChange={(event) =>
                patch({ maxWaitMs: toOptionalNumber(event.target.value) })
              }
            />
            <p className="text-xs leading-relaxed text-fg-2">
              {t("providerForm.proxyRateLimitMaxWaitHint", {
                defaultValue:
                  "超过这个时间仍未轮到就直接放行，避免本地排队把请求卡死。",
              })}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
