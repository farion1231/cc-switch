import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Activity, Copy, Edit, Loader2, Trash2 } from "lucide-react";
import { findVendorPreset } from "@/config/piProviderPresets";

interface PiProviderListProps {
  providers: Record<string, unknown>;
  onEdit: (providerId: string) => void;
  onDuplicate?: (providerId: string) => void;
  onDelete?: (providerId: string) => void;
  onTestConnectivity?: (providerId: string) => Promise<void>;
}

const BUILTIN_PROVIDER_IDS = new Set([
  "anthropic",
  "openai",
  "google",
  "openrouter",
  "deepseek",
  "xai",
  "mistral",
  "groq",
  "cerebras",
  "nvidia",
  "together",
  "fireworks",
  "huggingface",
  "kimi-coding",
  "minimax",
  "minimax-cn",
  "xiaomi",
]);

export function PiProviderList({
  providers,
  onEdit,
  onDuplicate,
  onDelete,
  onTestConnectivity,
}: PiProviderListProps) {
  const { t } = useTranslation();
  const entries = Object.entries(providers);
  const builtin = entries.filter(([id]) => BUILTIN_PROVIDER_IDS.has(id));
  const custom = entries.filter(([id]) => !BUILTIN_PROVIDER_IDS.has(id));

  if (entries.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-border p-8 text-center">
        <p className="text-sm text-muted-foreground">{t("pi.list.empty")}</p>
        <p className="mt-1.5 text-xs text-muted-foreground">
          {t("pi.list.emptyHint")}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {custom.length > 0 && (
        <ProviderGroup
          title={t("pi.list.custom")}
          entries={custom}
          onEdit={onEdit}
          onDuplicate={onDuplicate}
          onDelete={onDelete}
          onTestConnectivity={onTestConnectivity}
        />
      )}
      {builtin.length > 0 && (
        <ProviderGroup
          title={t("pi.list.builtin")}
          entries={builtin}
          onEdit={onEdit}
          onDuplicate={onDuplicate}
          onDelete={onDelete}
          onTestConnectivity={onTestConnectivity}
        />
      )}
    </div>
  );
}

function ProviderGroup({
  title,
  entries,
  onEdit,
  onDuplicate,
  onDelete,
  onTestConnectivity,
}: {
  title: string;
  entries: [string, unknown][];
  onEdit: (providerId: string) => void;
  onDuplicate?: (providerId: string) => void;
  onDelete?: (providerId: string) => void;
  onTestConnectivity?: (providerId: string) => Promise<void>;
}) {
  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold text-muted-foreground">{title}</h3>
      <div className="space-y-3">
        {entries.map(([id, config]) => (
          <PiProviderCard
            key={id}
            id={id}
            config={config}
            onEdit={onEdit}
            onDuplicate={onDuplicate}
            onDelete={onDelete}
            onTestConnectivity={onTestConnectivity}
          />
        ))}
      </div>
    </section>
  );
}

function PiProviderCard({
  id,
  config,
  onEdit,
  onDuplicate,
  onDelete,
  onTestConnectivity,
}: {
  id: string;
  config: unknown;
  onEdit: (providerId: string) => void;
  onDuplicate?: (providerId: string) => void;
  onDelete?: (providerId: string) => void;
  onTestConnectivity?: (providerId: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [isTesting, setIsTesting] = useState(false);
  const cfg = config as Record<string, unknown> | undefined;
  const vendor = findVendorPreset(id);
  const api = (cfg?.api as string) ?? "—";
  const baseUrl = (cfg?.baseUrl as string) ?? "";
  const models = Array.isArray(cfg?.models) ? cfg.models : [];
  const modelCount = models.length;
  const modelIds = models
    .slice(0, 4)
    .map((m: Record<string, unknown>) => (m.id as string) || "")
    .filter(Boolean);

  const handleTest = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!onTestConnectivity || isTesting) return;
    setIsTesting(true);
    try {
      await onTestConnectivity(id);
    } finally {
      setIsTesting(false);
    }
  };

  const iconButtonClass = "h-8 w-8 p-1";

  return (
    <div
      className={cn(
        "relative rounded-xl border border-border p-4 transition-all duration-300",
        "bg-card text-card-foreground",
        "hover:border-blue-500/50 hover:shadow-sm",
      )}
    >
      {/* Row 1: Provider name + badges + action buttons */}
      <div className="flex items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2 min-w-0">
          <h3 className="text-base font-semibold leading-none">
            {vendor?.name ?? id}
          </h3>
          <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
            {api}
          </Badge>
          {modelCount > 0 && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0">
              {t("pi.list.models", { count: modelCount })}
            </Badge>
          )}
        </div>

        {/* Action buttons — always visible */}
        <div className="flex items-center gap-1 flex-shrink-0">
          <Button
            size="icon"
            variant="ghost"
            onClick={() => onEdit(id)}
            title={t("pi.list.edit")}
            className={iconButtonClass}
          >
            <Edit className="h-4 w-4" />
          </Button>

          <Button
            size="icon"
            variant="ghost"
            onClick={() => onDuplicate?.(id)}
            title={t("pi.list.duplicate")}
            className={cn(
              iconButtonClass,
              !onDuplicate &&
                "opacity-40 cursor-not-allowed text-muted-foreground",
            )}
            disabled={!onDuplicate}
          >
            <Copy className="h-4 w-4" />
          </Button>

          <Button
            size="icon"
            variant="ghost"
            onClick={handleTest}
            disabled={isTesting || !onTestConnectivity || !baseUrl}
            title={t("pi.list.testConnectivity")}
            className={cn(
              iconButtonClass,
              (!onTestConnectivity || !baseUrl) &&
                "opacity-40 cursor-not-allowed text-muted-foreground",
            )}
          >
            {isTesting ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Activity className="h-4 w-4" />
            )}
          </Button>

          <Button
            size="icon"
            variant="ghost"
            onClick={() => onDelete?.(id)}
            title={t("pi.list.delete")}
            className={cn(
              iconButtonClass,
              onDelete
                ? "hover:text-red-500 dark:hover:text-red-400"
                : "opacity-40 cursor-not-allowed text-muted-foreground",
            )}
            disabled={!onDelete}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Row 2: Highlighted URL */}
      {baseUrl && (
        <div className="mt-2">
          <code className="text-xs font-mono px-1.5 py-0.5 rounded bg-blue-50 text-blue-700 dark:bg-blue-950/50 dark:text-blue-300">
            {baseUrl}
          </code>
        </div>
      )}

      {/* Row 3: Highlighted model IDs */}
      {modelIds.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {modelIds.map((modelId) => (
            <code
              key={modelId}
              className="text-[11px] font-mono px-1.5 py-0.5 rounded bg-emerald-50 text-emerald-700 dark:bg-emerald-950/50 dark:text-emerald-300"
            >
              {modelId}
            </code>
          ))}
          {modelCount > 4 && (
            <span className="text-[11px] text-muted-foreground self-center">
              {t("pi.list.moreModels", { count: modelCount - 4 })}
            </span>
          )}
        </div>
      )}
    </div>
  );
}
