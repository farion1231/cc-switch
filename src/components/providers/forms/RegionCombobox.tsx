import React from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronsUpDown, Plus } from "lucide-react";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

/** 常见的 AWS 区域（与 IdC 探测列表保持一致的优先项） */
export const COMMON_AWS_REGIONS = [
  "us-east-1",
  "af-south-1",
  "ap-east-1",
  "ap-northeast-1",
  "ap-northeast-2",
  "ap-northeast-3",
  "ap-south-1",
  "ap-south-2",
  "ap-southeast-1",
  "ap-southeast-2",
  "ap-southeast-3",
  "ap-southeast-4",
  "ca-central-1",
  "ca-west-1",
  "eu-central-1",
  "eu-central-2",
  "eu-north-1",
  "eu-south-1",
  "eu-south-2",
  "eu-west-1",
  "eu-west-2",
  "eu-west-3",
  "il-central-1",
  "me-central-1",
  "me-south-1",
  "sa-east-1",
  "us-east-2",
  "us-west-1",
  "us-west-2",
  "us-gov-west-1",
  "us-gov-east-1",
];

interface RegionComboboxProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  id?: string;
  options?: string[];
}

/**
 * AWS Region 选择器：可下拉选择常见区域，也支持手动输入任意区域（allow-create）。
 */
export const RegionCombobox: React.FC<RegionComboboxProps> = ({
  value,
  onChange,
  disabled,
  id,
  options = COMMON_AWS_REGIONS,
}) => {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const [search, setSearch] = React.useState("");

  const query = search.trim();
  const lowerQuery = query.toLowerCase();
  const filtered = options.filter((r) => r.toLowerCase().includes(lowerQuery));
  // 输入的内容不在已有选项中（忽略大小写）时，提供「使用自定义」创建项
  const showCreate =
    query.length > 0 && !options.some((r) => r.toLowerCase() === lowerQuery);

  const commit = (region: string) => {
    onChange(region);
    setOpen(false);
    setSearch("");
  };

  return (
    <Popover
      modal
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setSearch("");
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          id={id}
          role="combobox"
          aria-expanded={open}
          disabled={disabled}
          className="flex w-full h-9 items-center justify-between whitespace-nowrap rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm ring-offset-background focus:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span className={cn("truncate", !value && "text-muted-foreground")}>
            {value || t("kiro.region", "Region (区域)")}
          </span>
          <ChevronsUpDown className="h-3.5 w-3.5 opacity-50 shrink-0 ml-1" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        sideOffset={6}
        avoidCollisions
        collisionPadding={8}
        className="z-[1000] w-[var(--radix-popover-trigger-width)] p-0"
      >
        <Command shouldFilter={false}>
          <CommandInput
            placeholder={t("kiro.searchRegion", "搜索或输入区域...")}
            value={search}
            onValueChange={setSearch}
          />
          <CommandList>
            {filtered.length === 0 && !showCreate && (
              <CommandEmpty>
                {t("kiro.noRegionMatch", "无匹配区域")}
              </CommandEmpty>
            )}
            {filtered.length > 0 && (
              <CommandGroup>
                {filtered.map((region) => (
                  <CommandItem
                    key={region}
                    value={region}
                    onSelect={() => commit(region)}
                  >
                    <Check
                      className={cn(
                        "mr-2 h-4 w-4",
                        value === region ? "opacity-100" : "opacity-0",
                      )}
                    />
                    {region}
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
            {showCreate && (
              <CommandGroup>
                <CommandItem value={query} onSelect={() => commit(query)}>
                  <Plus className="mr-2 h-4 w-4" />
                  {t("kiro.useCustomRegion", {
                    region: query,
                    defaultValue: `使用自定义区域 "${query}"`,
                  })}
                </CommandItem>
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};

export default RegionCombobox;
