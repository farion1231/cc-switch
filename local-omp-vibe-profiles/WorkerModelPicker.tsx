import { useRef, useState } from "react";
import { Check, ChevronDown, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { fieldClass } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export type WorkerSelector = string | string[] | null;
export type CatalogModel = {
  selector: string;
  provider: string;
  name: string;
  thinking: string[];
};
export type CatalogState = {
  models: CatalogModel[];
  loading: boolean;
  error: string;
};

export function WorkerModelPicker({
  title,
  value,
  inherit,
  emptyLabel = `继承 ${inherit}`,
  roles,
  catalog,
  onRefresh,
  onChange,
  disabled,
}: {
  title: string;
  value: WorkerSelector;
  inherit?: string;
  emptyLabel?: string;
  roles: Record<string, string>;
  catalog: CatalogState;
  onRefresh: () => void;
  onChange: (value: WorkerSelector) => void;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const raw = typeof value === "string" ? value : undefined;
  const exact = catalog.models.find((model) => model.selector === raw);
  const matched =
    exact ??
    catalog.models.find((model) =>
      model.thinking.some((level) => raw === `${model.selector}:${level}`),
    );
  const effort =
    matched && !exact ? raw?.slice(matched.selector.length + 1) : undefined;
  const label = Array.isArray(value)
    ? value.join(" → ")
    : (value ?? emptyLabel);
  const groups = new Map<string, CatalogModel[]>();
  if (open) {
    for (const model of catalog.models) {
      const group = groups.get(model.provider);
      if (group) group.push(model);
      else groups.set(model.provider, [model]);
    }
  }

  function pick(next: WorkerSelector) {
    onChange(next);
    setOpen(false);
  }

  return (
    <div className="grid min-w-0 grid-cols-[minmax(0,1fr)_7.5rem] items-center gap-x-2 gap-y-1 py-2 sm:grid-cols-[7rem_minmax(0,1fr)_7.5rem]">
      <div className="col-span-2 text-body font-medium sm:col-span-1">
        {title}
      </div>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            disabled={disabled}
            className={`${fieldClass} ${matched?.thinking.length ? "" : "col-span-2"} flex h-control min-w-0 items-center justify-between gap-2 text-start`}
            aria-label={`${title} 模型：${label}`}
          >
            <span className="min-w-0 truncate" title={label}>
              {(matched?.selector ?? label) || "空选择器列表"}
            </span>
            <ChevronDown
              className="h-4 w-4 shrink-0 opacity-50"
              aria-hidden="true"
            />
          </button>
        </PopoverTrigger>
        <PopoverContent
          aria-label={`${title} 模型选择`}
          className="w-[min(420px,calc(100vw-1.5rem))] p-0"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            input.current?.focus();
          }}
        >
          <Command
            label={`${title} 模型目录`}
            filter={(value, search, keywords) => {
              const haystack =
                `${value} ${keywords?.join(" ") ?? ""}`.toLowerCase();
              return search
                .trim()
                .toLowerCase()
                .split(/\s+/)
                .every((part) => haystack.includes(part))
                ? 1
                : 0;
            }}
          >
            <CommandInput ref={input} placeholder="搜索模型、供应商或 @角色" />
            <CommandList>
              <CommandEmpty>没有匹配的模型或角色</CommandEmpty>
              <CommandGroup heading="原生路由">
                <CommandItem
                  value={emptyLabel}
                  onSelect={() => pick(null)}
                >
                  <span className="flex-1">{emptyLabel}</span>
                  {value === null && <Check aria-label="已选" />}
                </CommandItem>
                {Object.entries(roles).map(([role, selector]) => (
                  <CommandItem
                    key={role}
                    value={`@${role}`}
                    onSelect={() => pick(`@${role}`)}
                  >
                    <span className="shrink-0 font-mono">@{role}</span>
                    <span
                      className="min-w-0 flex-1 truncate text-xs text-fg-2"
                      title={selector}
                    >
                      {selector}
                    </span>
                    {raw === `@${role}` && <Check aria-label="已选" />}
                  </CommandItem>
                ))}
              </CommandGroup>
              {Array.from(groups, ([provider, models]) => (
                <CommandGroup key={provider} heading={provider}>
                  {models.map((model) => (
                    <CommandItem
                      key={model.selector}
                      value={model.selector}
                      keywords={[model.name, provider]}
                      onSelect={() =>
                        pick(model === matched ? value : model.selector)
                      }
                    >
                      <span
                        className="min-w-0 flex-1 truncate font-mono"
                        title={model.selector}
                      >
                        {model.name}
                      </span>
                      {matched === model && <Check aria-label="已选" />}
                    </CommandItem>
                  ))}
                </CommandGroup>
              ))}
            </CommandList>
          </Command>
          <div className="flex items-center justify-between gap-2 border-t border-border px-2 py-1">
            <span role="status" className="min-w-0 text-xs text-fg-2">
              {catalog.loading
                ? "正在读取 OMP 模型目录…"
                : catalog.error || `${catalog.models.length} 个模型`}
            </span>
            <Button
              variant="quiet"
              size="icon-compact"
              title="刷新模型目录"
              aria-label="刷新模型目录"
              disabled={catalog.loading}
              onClick={onRefresh}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          </div>
        </PopoverContent>
      </Popover>
      {matched && matched.thinking.length > 0 && (
        <Select
          value={effort ?? "inherit"}
          disabled={disabled}
          onValueChange={(level) =>
            onChange(
              level === "inherit"
                ? matched.selector
                : `${matched.selector}:${level}`,
            )
          }
        >
          <SelectTrigger aria-label={`${title} 推理等级`} className="min-w-0">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="inherit">继承推理</SelectItem>
            {matched.thinking.map((level) => (
              <SelectItem key={level} value={level}>
                {level}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </div>
  );
}
