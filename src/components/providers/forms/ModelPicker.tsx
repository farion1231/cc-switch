import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { modelsApi, type ModelInfo } from "@/lib/api/providers";
import { toast } from "sonner";

interface ModelPickerProps {
  baseUrl: string;
  apiKey?: string;
  value: string;
  onChange: (model: string) => void;
  placeholder?: string;
  disabled?: boolean;
}

export function ModelPicker({
  baseUrl,
  apiKey,
  value,
  onChange,
  placeholder,
  disabled,
}: ModelPickerProps) {
  const { t } = useTranslation();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");

  const fetchModels = useCallback(async () => {
    if (!baseUrl?.trim()) {
      return;
    }

    setLoading(true);
    setError(null);

    try {
      const response = await modelsApi.fetch(baseUrl, apiKey);
      setModels(response.models);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      toast.error(
        t("modelPicker.fetchError", {
          defaultValue: "Failed to fetch models: {{error}}",
          error: message,
        }),
      );
    } finally {
      setLoading(false);
    }
  }, [baseUrl, apiKey, t]);

  useEffect(() => {
    if (baseUrl?.trim()) {
      fetchModels();
    }
  }, [baseUrl, apiKey, fetchModels]);

  const filteredModels = models.filter((m) =>
    m.id.toLowerCase().includes(search.toLowerCase()),
  );

  const handleSelect = (modelId: string) => {
    onChange(modelId);
  };

  if (!baseUrl?.trim()) {
    return (
      <Input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={
          placeholder ||
          t("modelPicker.enterBaseUrl", {
            defaultValue: "Enter base URL to load models",
          })
        }
        disabled={disabled}
      />
    );
  }

  if (loading) {
    return (
      <div className="flex items-center gap-2">
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          className="flex-1"
        />
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (error && models.length === 0) {
    return (
      <div className="flex items-center gap-2">
        <Input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          className="flex-1"
        />
        <Button
          variant="outline"
          size="icon"
          onClick={fetchModels}
          disabled={disabled}
          title={t("modelPicker.retry", { defaultValue: "Retry" })}
        >
          <RefreshCw className="h-4 w-4" />
        </Button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <Select value={value} onValueChange={handleSelect} disabled={disabled}>
        <SelectTrigger className="flex-1">
          <SelectValue
            placeholder={
              placeholder ||
              t("modelPicker.selectModel", { defaultValue: "Select a model" })
            }
          />
        </SelectTrigger>
        <SelectContent>
          <div className="p-2">
            <div className="relative">
              <Search className="absolute left-2 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder={t("modelPicker.search", {
                  defaultValue: "Search models...",
                })}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="pl-8"
              />
            </div>
          </div>
          <div className="max-h-60 overflow-auto">
            {filteredModels.length === 0 ? (
              <div className="p-2 text-center text-muted-foreground text-sm">
                {t("modelPicker.noModels", { defaultValue: "No models found" })}
              </div>
            ) : (
              filteredModels.map((model) => (
                <SelectItem key={model.id} value={model.id}>
                  <div className="flex flex-col">
                    <span>{model.id}</span>
                    {model.owned_by && (
                      <span className="text-xs text-muted-foreground">
                        {model.owned_by}
                      </span>
                    )}
                  </div>
                </SelectItem>
              ))
            )}
          </div>
        </SelectContent>
      </Select>
      <Button
        variant="outline"
        size="icon"
        onClick={fetchModels}
        disabled={disabled || loading}
        title={t("modelPicker.refresh", { defaultValue: "Refresh models" })}
      >
        <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
      </Button>
    </div>
  );
}
