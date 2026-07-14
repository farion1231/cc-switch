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

  // 有模型数据: Input + DropdownMenu
  if (fetchedModels.length > 0) {
    const grouped: Record<string, FetchedModel[]> = {};
    for (const model of fetchedModels) {
      const vendor = model.ownedBy || "Other";
      if (!grouped[vendor]) grouped[vendor] = [];
      grouped[vendor].push(model);
    }
    const vendors = Object.keys(grouped).sort();

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
            {vendors.map((vendor, vi) => (
              <div key={vendor}>
                {vi > 0 && <DropdownMenuSeparator />}
                <DropdownMenuLabel>{vendor}</DropdownMenuLabel>
                {grouped[vendor].map((model) => (
                  <DropdownMenuItem
                    key={model.id}
                    onSelect={() => onChange(model.id)}
                  >
                    {model.id}
                  </DropdownMenuItem>
                ))}
              </div>
            ))}
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
