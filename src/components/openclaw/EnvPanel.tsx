import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Save, Eye, EyeOff } from "lucide-react";
import { toast } from "sonner";
import { useOpenClawEnv, useSaveOpenClawEnv } from "@/hooks/useOpenClaw";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { OpenClawEnvConfig } from "@/types";

interface EnvEntry {
  id: string;
  key: string;
  value: string;
  isNew?: boolean;
}

const EnvPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: envData, isLoading } = useOpenClawEnv();
  const saveEnvMutation = useSaveOpenClawEnv();
  const [entries, setEntries] = useState<EnvEntry[]>([]);
  const [visibleKeys, setVisibleKeys] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (envData) {
      const items: EnvEntry[] = Object.entries(envData).map(([key, value]) => ({
        id: crypto.randomUUID(),
        key,
        value: String(value ?? ""),
      }));
      setEntries(items.length > 0 ? items : []);
    }
  }, [envData]);

  const handleSave = async () => {
    try {
      const env: OpenClawEnvConfig = {};
      const seen = new Set<string>();
      for (const entry of entries) {
        const trimmedKey = entry.key.trim();
        if (trimmedKey) {
          if (seen.has(trimmedKey)) {
            toast.error(t("openclaw.env.duplicateKey", { key: trimmedKey }));
            return;
          }
          seen.add(trimmedKey);
          env[trimmedKey] = entry.value;
        }
      }
      await saveEnvMutation.mutateAsync(env);
      toast.success(t("openclaw.env.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(t("openclaw.env.saveFailed"), {
        description: detail || undefined,
      });
    }
  };

  const addEntry = () => {
    setEntries((prev) => [
      ...prev,
      { id: crypto.randomUUID(), key: "", value: "", isNew: true },
    ]);
  };

  const removeEntry = (index: number) => {
    setEntries((prev) => prev.filter((_, i) => i !== index));
  };

  const updateEntry = (index: number, field: "key" | "value", val: string) => {
    setEntries((prev) =>
      prev.map((entry, i) =>
        i === index ? { ...entry, [field]: val } : entry,
      ),
    );
  };

  const toggleVisibility = (key: string) => {
    setVisibleKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const isApiKey = (key: string) => /key|token|secret|password/i.test(key);

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-4">
        {t("openclaw.env.description")}
      </p>

      <div className="space-y-3">
        {entries.map((entry, index) => {
          const sensitive = isApiKey(entry.key);
          const visibilityId = entry.key || `__new_${index}`;
          const visible = visibleKeys.has(visibilityId);

          return (
            <div key={entry.id} className="flex items-center gap-2">
              <div className="w-[200px] flex-shrink-0">
                <Input
                  value={entry.key}
                  onChange={(e) => updateEntry(index, "key", e.target.value)}
                  placeholder={t("openclaw.env.keyPlaceholder")}
                  className="font-mono text-xs"
                  autoFocus={entry.isNew}
                />
              </div>
              <div className="flex-1 flex items-center gap-1">
                <Input
                  type={sensitive && !visible ? "password" : "text"}
                  value={entry.value}
                  onChange={(e) => updateEntry(index, "value", e.target.value)}
                  placeholder={t("openclaw.env.valuePlaceholder")}
                  className="font-mono text-xs"
                />
                {sensitive && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="flex-shrink-0 h-9 w-9 text-muted-foreground"
                    onClick={() => toggleVisibility(visibilityId)}
                  >
                    {visible ? (
                      <EyeOff className="w-4 h-4" />
                    ) : (
                      <Eye className="w-4 h-4" />
                    )}
                  </Button>
                )}
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeEntry(index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          );
        })}
      </div>

      <div className="flex items-center gap-2 mt-4">
        <Button variant="outline" size="sm" onClick={addEntry}>
          <Plus className="w-4 h-4 mr-1" />
          {t("openclaw.env.add")}
        </Button>
        <div className="flex-1" />
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveEnvMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveEnvMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default EnvPanel;
