import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ChevronDown, Download, Loader2 } from "lucide-react";
import type { FetchedModel } from "@/lib/api/model-fetch";

interface ModelInputWithFetchProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  fetchedModels: FetchedModel[];
  isLoading: boolean;
  /** 传入时显示获取按钮；不传时只在有数据后显示下拉 */
  onFetch?: () => void;
}

export function ModelInputWithFetch({
  id,
  value,
  onChange,
  placeholder,
  fetchedModels,
  isLoading,
  onFetch,
}: ModelInputWithFetchProps) {
  const { t } = useTranslation();

  // 模型搜索
  const [modelSearch, setModelSearch] = useState("");

  // 有模型数据: Input + DropdownMenu
  if (fetchedModels.length > 0) {
    const grouped: Record<string, FetchedModel[]> = {};
    for (const model of fetchedModels) {
      const vendor = model.ownedBy || "Other";
      if (!grouped[vendor]) grouped[vendor] = [];
      grouped[vendor].push(model);
    }

    // 根据搜索过滤模型
    const filteredGrouped: Record<string, FetchedModel[]> = {};
    if (modelSearch.trim()) {
      const search = modelSearch.toLowerCase();
      for (const [vendor, models] of Object.entries(grouped)) {
        const filtered = models.filter((m) =>
          m.id.toLowerCase().includes(search),
        );
        if (filtered.length > 0) filteredGrouped[vendor] = filtered;
      }
    } else {
      Object.assign(filteredGrouped, grouped);
    }
    const filteredVendors = Object.keys(filteredGrouped).sort();

    return (
      <div className="flex gap-1">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          className="flex-1"
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="icon" className="shrink-0">
              <ChevronDown className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            className="max-h-64 overflow-y-auto z-[200]"
          >
            <div className="px-2 py-1.5">
              <Input
                value={modelSearch}
                onChange={(e) => setModelSearch(e.target.value)}
                placeholder={t("providerForm.searchModels", {
                  defaultValue: "搜索模型...",
                })}
                autoComplete="off"
                className="h-7 text-xs"
                onKeyDown={(e) => {
                  e.stopPropagation();
                }}
              />
            </div>
            <DropdownMenuSeparator />
            {filteredVendors.length === 0 ? (
              <DropdownMenuItem disabled>
                {t("providerForm.noModelsFound", {
                  defaultValue: "未找到模型",
                })}
              </DropdownMenuItem>
            ) : (
              filteredVendors.map((vendor, vi) => (
                <div key={vendor}>
                  {vi > 0 && <DropdownMenuSeparator />}
                  <DropdownMenuLabel>{vendor}</DropdownMenuLabel>
                  {filteredGrouped[vendor].map((model) => (
                    <DropdownMenuItem
                      key={model.id}
                      onSelect={() => {
                        onChange(model.id);
                        setModelSearch("");
                      }}
                    >
                      {model.id}
                    </DropdownMenuItem>
                  ))}
                </div>
              ))
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    );
  }

  // 加载中: Input + Spinner
  if (isLoading) {
    return (
      <div className="flex gap-1">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          className="flex-1"
        />
        <Button variant="outline" size="icon" className="shrink-0" disabled>
          <Loader2 className="h-4 w-4 animate-spin" />
        </Button>
      </div>
    );
  }

  // 有 onFetch: Input + 获取按钮
  if (onFetch) {
    return (
      <div className="flex gap-1">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          className="flex-1"
        />
        <Button
          variant="outline"
          size="icon"
          className="shrink-0"
          type="button"
          onClick={onFetch}
          title={t("providerForm.fetchModels")}
        >
          <Download className="h-4 w-4" />
        </Button>
      </div>
    );
  }

  // 无 onFetch: 纯 Input
  return (
    <Input
      id={id}
      type="text"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      autoComplete="off"
    />
  );
}
