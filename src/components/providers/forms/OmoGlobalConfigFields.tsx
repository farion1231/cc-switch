import {
  useState,
  useEffect,
  useCallback,
  forwardRef,
  useImperativeHandle,
} from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Save,
  Loader2,
  X,
  FolderInput,
  RotateCcw,
  ChevronsUpDown,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import type { OmoGlobalConfig } from "@/types/omo";
import {
  OMO_DISABLEABLE_AGENTS,
  OMO_DISABLEABLE_MCPS,
  OMO_DISABLEABLE_HOOKS,
  OMO_DISABLEABLE_SKILLS,
  OMO_DEFAULT_SCHEMA_URL,
  OMO_SISYPHUS_AGENT_PLACEHOLDER,
  OMO_LSP_PLACEHOLDER,
  OMO_EXPERIMENTAL_PLACEHOLDER,
  OMO_BACKGROUND_TASK_PLACEHOLDER,
  OMO_BROWSER_AUTOMATION_PLACEHOLDER,
  OMO_CLAUDE_CODE_PLACEHOLDER,
  OMO_SLIM_DISABLEABLE_AGENTS,
  OMO_SLIM_DISABLEABLE_MCPS,
  OMO_SLIM_DISABLEABLE_HOOKS,
  OMO_SLIM_DEFAULT_SCHEMA_URL,
} from "@/types/omo";
import {
  useOmoGlobalConfig,
  useSaveOmoGlobalConfig,
  useReadOmoLocalFile,
  useOmoSlimGlobalConfig,
  useSaveOmoSlimGlobalConfig,
  useReadOmoSlimLocalFile,
} from "@/lib/query/omo";

interface PresetOption {
  readonly value: string;
  readonly label: string;
}

export interface OmoGlobalConfigFieldsRef {
  buildCurrentConfig: () => OmoGlobalConfig;
  buildCurrentConfigStrict: () => OmoGlobalConfig;
  importFromLocal: () => Promise<void>;
}

interface OmoGlobalConfigFieldsProps {
  onStateChange?: (config: OmoGlobalConfig) => void;
  hideSaveButtons?: boolean;
  isSlim?: boolean;
}

type OmoAdvancedFieldKey =
  | "lspStr"
  | "experimentalStr"
  | "backgroundTaskStr"
  | "browserStr"
  | "claudeCodeStr";

const OMO_ADVANCED_JSON_FIELDS: ReadonlyArray<{
  key: OmoAdvancedFieldKey;
  labelKey: string;
  defaultLabel: string;
  placeholder: string;
  minHeight: string;
}> = [
  {
    key: "lspStr",
    labelKey: "omo.advancedLsp",
    defaultLabel: "LSP Config",
    placeholder: OMO_LSP_PLACEHOLDER,
    minHeight: "200px",
  },
  {
    key: "experimentalStr",
    labelKey: "omo.advancedExperimental",
    defaultLabel: "Experimental Features",
    placeholder: OMO_EXPERIMENTAL_PLACEHOLDER,
    minHeight: "120px",
  },
  {
    key: "backgroundTaskStr",
    labelKey: "omo.advancedBackgroundTask",
    defaultLabel: "Background Tasks",
    placeholder: OMO_BACKGROUND_TASK_PLACEHOLDER,
    minHeight: "250px",
  },
  {
    key: "browserStr",
    labelKey: "omo.advancedBrowserAutomation",
    defaultLabel: "Browser Automation",
    placeholder: OMO_BROWSER_AUTOMATION_PLACEHOLDER,
    minHeight: "80px",
  },
  {
    key: "claudeCodeStr",
    labelKey: "omo.advancedClaudeCode",
    defaultLabel: "Claude Code",
    placeholder: OMO_CLAUDE_CODE_PLACEHOLDER,
    minHeight: "180px",
  },
];

const OMO_SLIM_ADVANCED_KEYS: ReadonlySet<OmoAdvancedFieldKey> = new Set([
  "lspStr",
  "experimentalStr",
]);

function TagListEditor({
  label,
  values,
  onChange,
  placeholder,
  presets,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
  placeholder?: string;
  presets?: readonly PresetOption[];
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");

  const toggleValue = (v: string) => {
    if (values.includes(v)) {
      onChange(values.filter((x) => x !== v));
    } else {
      onChange([...values, v]);
    }
  };
  const customValue = search.trim();
  const canAddCustom = customValue.length > 0 && !values.includes(customValue);
  const triggerText =
    values.length === 0
      ? placeholder || t("omo.selectPlaceholder", { defaultValue: "Select..." })
      : values.length === 1
        ? values[0]
        : `${values[0]} +${values.length - 1}`;

  const availablePresets = presets?.filter(
    (p) =>
      !search.trim() ||
      p.label.toLowerCase().includes(search.toLowerCase()) ||
      p.value.toLowerCase().includes(search.toLowerCase()),
  );

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <Label className="text-sm">{label}</Label>
        {values.length > 0 && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-6 px-1.5 text-xs text-muted-foreground"
            onClick={() => onChange([])}
          >
            {t("omo.clear", { defaultValue: "Clear" })}
          </Button>
        )}
      </div>
      {values.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {values.map((v, i) => (
            <Badge
              key={`${v}-${i}`}
              variant="secondary"
              className="text-xs gap-1"
            >
              {v}
              <button
                type="button"
                onClick={() => onChange(values.filter((_, idx) => idx !== i))}
                className="hover:text-destructive"
              >
                <X className="h-3 w-3" />
              </button>
            </Badge>
          ))}
        </div>
      )}
      <DropdownMenu open={open} onOpenChange={setOpen} modal={false}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            className={cn(
              "flex items-center justify-between w-full h-8 px-3 rounded-md border border-input bg-background text-sm",
              "hover:bg-accent hover:text-accent-foreground transition-colors",
              open && "ring-2 ring-ring",
            )}
            aria-expanded={open}
          >
            <span
              className={cn(
                "truncate",
                values.length > 0 ? "text-foreground" : "text-muted-foreground",
              )}
            >
              {triggerText}
            </span>
            <ChevronsUpDown className="h-4 w-4 shrink-0 text-muted-foreground" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="start"
          sideOffset={6}
          className="w-[var(--radix-dropdown-menu-trigger-width)] p-0 z-[120]"
        >
          <div className="p-1.5 border-b border-border/30">
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter" && canAddCustom) {
                  e.preventDefault();
                  onChange([...values, customValue]);
                  setSearch("");
                }
              }}
              placeholder={
                placeholder ||
                t("omo.searchOrType", {
                  defaultValue: "Search or type custom value...",
                })
              }
              className="h-7 text-sm"
              autoFocus
            />
          </div>
          {canAddCustom && (
            <button
              type="button"
              className="w-full px-2.5 py-1.5 text-left text-sm border-b border-border/30 hover:bg-accent"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                onChange([...values, customValue]);
                setSearch("");
              }}
            >
              + {customValue}
            </button>
          )}
          <div className="max-h-48 overflow-auto py-1">
            {availablePresets && availablePresets.length > 0 ? (
              availablePresets.map((p) => {
                const checked = values.includes(p.value);
                return (
                  <DropdownMenuCheckboxItem
                    key={p.value}
                    checked={checked}
                    onSelect={(e) => e.preventDefault()}
                    onCheckedChange={() => toggleValue(p.value)}
                    className="text-sm"
                  >
                    {p.label}
                  </DropdownMenuCheckboxItem>
                );
              })
            ) : (
              <div className="px-2.5 py-2 text-sm text-muted-foreground">
                {t("omo.noMatches", { defaultValue: "No matches" })}
              </div>
            )}
          </div>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function JsonTextareaField({
  label,
  value,
  onChange,
  placeholder,
  minHeight,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  minHeight?: string;
}) {
  return (
    <div className="space-y-1.5">
      <Label className="text-sm">{label}</Label>
      <Textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder || "{}"}
        className="font-mono text-sm"
        style={{ minHeight: minHeight || "100px" }}
      />
    </div>
  );
}

export const OmoGlobalConfigFields = forwardRef<
  OmoGlobalConfigFieldsRef,
  OmoGlobalConfigFieldsProps
>(function OmoGlobalConfigFields(
  { onStateChange, hideSaveButtons, isSlim = false },
  ref,
) {
  const { t } = useTranslation();
  const { data: standardConfig } = useOmoGlobalConfig(!isSlim);
  const { data: slimConfig } = useOmoSlimGlobalConfig(isSlim);
  const config = isSlim ? slimConfig : standardConfig;
  const standardSaveMutation = useSaveOmoGlobalConfig();
  const slimSaveMutation = useSaveOmoSlimGlobalConfig();
  const saveMutation = isSlim ? slimSaveMutation : standardSaveMutation;
  const standardReadLocal = useReadOmoLocalFile();
  const slimReadLocal = useReadOmoSlimLocalFile();

  const defaultSchemaUrl = isSlim
    ? OMO_SLIM_DEFAULT_SCHEMA_URL
    : OMO_DEFAULT_SCHEMA_URL;

  const [schemaUrl, setSchemaUrl] = useState(defaultSchemaUrl);
  const [sisyphusAgentStr, setSisyphusAgentStr] = useState("");
  const [disabledAgents, setDisabledAgents] = useState<string[]>([]);
  const [disabledMcps, setDisabledMcps] = useState<string[]>([]);
  const [disabledHooks, setDisabledHooks] = useState<string[]>([]);
  const [disabledSkills, setDisabledSkills] = useState<string[]>([]);
  const [lspStr, setLspStr] = useState("");
  const [experimentalStr, setExperimentalStr] = useState("");
  const [backgroundTaskStr, setBackgroundTaskStr] = useState("");
  const [browserStr, setBrowserStr] = useState("");
  const [claudeCodeStr, setClaudeCodeStr] = useState("");
  const [otherFieldsStr, setOtherFieldsStr] = useState("");
  const [loaded, setLoaded] = useState(false);

  const applyGlobalState = useCallback((global: OmoGlobalConfig) => {
    setSchemaUrl(global.schemaUrl || defaultSchemaUrl);
    setSisyphusAgentStr(
      global.sisyphusAgent ? JSON.stringify(global.sisyphusAgent, null, 2) : "",
    );
    setDisabledAgents(global.disabledAgents || []);
    setDisabledMcps(global.disabledMcps || []);
    setDisabledHooks(global.disabledHooks || []);
    setDisabledSkills(global.disabledSkills || []);
    setLspStr(global.lsp ? JSON.stringify(global.lsp, null, 2) : "");
    setExperimentalStr(
      global.experimental ? JSON.stringify(global.experimental, null, 2) : "",
    );
    setBackgroundTaskStr(
      global.backgroundTask
        ? JSON.stringify(global.backgroundTask, null, 2)
        : "",
    );
    setBrowserStr(
      global.browserAutomationEngine
        ? JSON.stringify(global.browserAutomationEngine, null, 2)
        : "",
    );
    setClaudeCodeStr(
      global.claudeCode ? JSON.stringify(global.claudeCode, null, 2) : "",
    );
    setOtherFieldsStr(
      global.otherFields ? JSON.stringify(global.otherFields, null, 2) : "",
    );
  }, []);

  useEffect(() => {
    if (config && !loaded) {
      applyGlobalState(config);
      setLoaded(true);
    }
  }, [config, loaded, applyGlobalState]);

  const parseJsonField = useCallback(
    (
      fieldName: string,
      raw: string,
      strict: boolean,
    ): Record<string, unknown> | undefined => {
      if (!raw.trim()) return undefined;
      try {
        const parsed: unknown = JSON.parse(raw);
        if (
          typeof parsed !== "object" ||
          parsed === null ||
          Array.isArray(parsed)
        ) {
          if (strict) {
            throw new Error(
              t("omo.jsonMustBeObject", {
                field: fieldName,
                defaultValue: "{{field}} must be a JSON object",
              }),
            );
          }
          return undefined;
        }
        return parsed as Record<string, unknown>;
      } catch (error) {
        if (strict) {
          if (error instanceof Error) {
            throw error;
          }
          throw new Error(
            t("omo.jsonInvalid", {
              field: fieldName,
              defaultValue: "{{field}} contains invalid JSON",
            }),
          );
        }
        return undefined;
      }
    },
    [t],
  );

  const buildCurrentConfigInternal = useCallback(
    (strict: boolean): OmoGlobalConfig => {
      return {
        id: "global",
        schemaUrl: schemaUrl || undefined,
        sisyphusAgent: parseJsonField(
          t("omo.sisyphusAgentConfig", {
            defaultValue: "Sisyphus Agent",
          }),
          sisyphusAgentStr,
          strict,
        ),
        disabledAgents,
        disabledMcps,
        disabledHooks,
        disabledSkills,
        lsp: parseJsonField(
          t("omo.advancedLsp", { defaultValue: "LSP" }),
          lspStr,
          strict,
        ),
        experimental: parseJsonField(
          t("omo.advancedExperimental", { defaultValue: "Experimental" }),
          experimentalStr,
          strict,
        ),
        backgroundTask: parseJsonField(
          t("omo.advancedBackgroundTask", {
            defaultValue: "Background Task",
          }),
          backgroundTaskStr,
          strict,
        ),
        browserAutomationEngine: parseJsonField(
          t("omo.advancedBrowserAutomation", {
            defaultValue: "Browser Automation",
          }),
          browserStr,
          strict,
        ),
        claudeCode: parseJsonField(
          t("omo.advancedClaudeCode", { defaultValue: "Claude Code" }),
          claudeCodeStr,
          strict,
        ),
        otherFields: parseJsonField(
          t("omo.otherFields", {
            defaultValue: "Other Config",
          }),
          otherFieldsStr,
          strict,
        ),
        updatedAt: new Date().toISOString(),
      };
    },
    [
      schemaUrl,
      sisyphusAgentStr,
      disabledAgents,
      disabledMcps,
      disabledHooks,
      disabledSkills,
      lspStr,
      experimentalStr,
      backgroundTaskStr,
      browserStr,
      claudeCodeStr,
      otherFieldsStr,
      parseJsonField,
    ],
  );

  const buildCurrentConfig = useCallback(
    () => buildCurrentConfigInternal(false),
    [buildCurrentConfigInternal],
  );

  const buildCurrentConfigStrict = useCallback(
    () => buildCurrentConfigInternal(true),
    [buildCurrentConfigInternal],
  );

  useEffect(() => {
    if (loaded && onStateChange) {
      onStateChange(buildCurrentConfig());
    }
  }, [loaded, onStateChange, buildCurrentConfig]);

  const handleSaveGlobal = useCallback(async () => {
    try {
      const result = buildCurrentConfigStrict();
      await saveMutation.mutateAsync(result);
      toast.success(
        t("omo.globalConfigSaved", {
          defaultValue: "Global config saved",
        }),
      );
    } catch (err) {
      toast.error(String(err));
    }
  }, [buildCurrentConfigStrict, saveMutation, t]);

  const disabledCount =
    disabledAgents.length +
    disabledMcps.length +
    disabledHooks.length +
    disabledSkills.length;
  const advancedFieldValues: Record<OmoAdvancedFieldKey, string> = {
    lspStr,
    experimentalStr,
    backgroundTaskStr,
    browserStr,
    claudeCodeStr,
  };

  const advancedFieldSetters: Record<
    OmoAdvancedFieldKey,
    (value: string) => void
  > = {
    lspStr: setLspStr,
    experimentalStr: setExperimentalStr,
    backgroundTaskStr: setBackgroundTaskStr,
    browserStr: setBrowserStr,
    claudeCodeStr: setClaudeCodeStr,
  };

  const disabledEditorConfigs = [
    {
      key: "agents",
      label: t("omo.disabledAgents", { defaultValue: "Agents" }),
      values: disabledAgents,
      onChange: setDisabledAgents,
      placeholder: t("omo.disabledAgentsPlaceholder", {
        defaultValue: "Disabled Agents",
      }),
      presets: isSlim ? OMO_SLIM_DISABLEABLE_AGENTS : OMO_DISABLEABLE_AGENTS,
    },
    {
      key: "mcps",
      label: t("omo.disabledMcps", { defaultValue: "MCPs" }),
      values: disabledMcps,
      onChange: setDisabledMcps,
      placeholder: t("omo.disabledMcpsPlaceholder", {
        defaultValue: "Disabled MCPs",
      }),
      presets: isSlim ? OMO_SLIM_DISABLEABLE_MCPS : OMO_DISABLEABLE_MCPS,
    },
    {
      key: "hooks",
      label: t("omo.disabledHooks", { defaultValue: "Hooks" }),
      values: disabledHooks,
      onChange: setDisabledHooks,
      placeholder: t("omo.disabledHooksPlaceholder", {
        defaultValue: "Disabled Hooks",
      }),
      presets: isSlim ? OMO_SLIM_DISABLEABLE_HOOKS : OMO_DISABLEABLE_HOOKS,
    },
    ...(!isSlim
      ? [
          {
            key: "skills" as const,
            label: t("omo.disabledSkills", { defaultValue: "Skills" }),
            values: disabledSkills,
            onChange: setDisabledSkills,
            placeholder: t("omo.disabledSkillsPlaceholder", {
              defaultValue: "Disabled Skills",
            }),
            presets: OMO_DISABLEABLE_SKILLS,
          },
        ]
      : []),
  ];

  const readLocalFile = isSlim ? slimReadLocal : standardReadLocal;

  const handleImportGlobalFromLocal = useCallback(async () => {
    try {
      const data = await readLocalFile.mutateAsync();
      applyGlobalState(data.global);
      toast.success(
        t("omo.importGlobalSuccess", {
          defaultValue: "Imported global config from local file (unsaved)",
        }),
      );
    } catch (err) {
      toast.error(
        t("omo.importGlobalFailed", {
          error: String(err),
          defaultValue: "Failed to read local file: {{error}}",
        }),
      );
    }
  }, [readLocalFile, applyGlobalState, t]);

  useImperativeHandle(
    ref,
    () => ({
      buildCurrentConfig,
      buildCurrentConfigStrict,
      importFromLocal: handleImportGlobalFromLocal,
    }),
    [buildCurrentConfig, buildCurrentConfigStrict, handleImportGlobalFromLocal],
  );

  return (
    <div className="space-y-4">
      {!hideSaveButtons && (
        <div className="flex items-center justify-end gap-1.5">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 text-xs"
            disabled={readLocalFile.isPending}
            onClick={handleImportGlobalFromLocal}
          >
            {readLocalFile.isPending ? (
              <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
            ) : (
              <FolderInput className="h-3.5 w-3.5 mr-1" />
            )}
            {t("omo.importLocal", { defaultValue: "Import Local" })}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            disabled={saveMutation.isPending}
            onClick={handleSaveGlobal}
          >
            {saveMutation.isPending ? (
              <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
            ) : (
              <Save className="h-3.5 w-3.5 mr-1" />
            )}
            {t("omo.saveGlobalConfig", { defaultValue: "Save Global Config" })}
          </Button>
        </div>
      )}

      <div className="space-y-1.5">
        <div className="flex items-center justify-between">
          <Label className="text-sm">
            {t("omo.schemaUrl", { defaultValue: "$schema" })}
          </Label>
          {schemaUrl !== OMO_DEFAULT_SCHEMA_URL && (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-6 text-xs px-1.5"
              onClick={() => setSchemaUrl(OMO_DEFAULT_SCHEMA_URL)}
            >
              <RotateCcw className="h-3 w-3 mr-0.5" />
              {t("omo.resetDefault", { defaultValue: "Reset" })}
            </Button>
          )}
        </div>
        <Input
          value={schemaUrl}
          onChange={(e) => setSchemaUrl(e.target.value)}
          placeholder={defaultSchemaUrl}
          className="text-sm h-8"
        />
      </div>

      {!isSlim && (
        <div className="rounded-md border border-border/40 bg-muted/10 p-2 space-y-2">
          <Label className="text-sm font-semibold">
            {t("omo.sisyphusAgentConfig", {
              defaultValue: "Sisyphus Agent",
            })}
          </Label>
          <Textarea
            value={sisyphusAgentStr}
            onChange={(e) => setSisyphusAgentStr(e.target.value)}
            placeholder={OMO_SISYPHUS_AGENT_PLACEHOLDER}
            className="font-mono text-sm"
            style={{ minHeight: "140px" }}
          />
        </div>
      )}

      <div className="rounded-md border border-border/40 bg-muted/10 p-2 space-y-3">
        <div className="flex items-center gap-2">
          <Label className="text-sm font-semibold">
            {t("omo.disabledItems", { defaultValue: "Disabled Items" })}
          </Label>
          {disabledCount > 0 && (
            <Badge variant="secondary" className="text-xs h-5">
              {disabledCount}
            </Badge>
          )}
        </div>
        {disabledEditorConfigs.map((editor) => (
          <TagListEditor
            key={editor.key}
            label={editor.label}
            values={editor.values}
            onChange={editor.onChange}
            placeholder={editor.placeholder}
            presets={editor.presets}
          />
        ))}
      </div>

      <div className="rounded-md border border-border/40 bg-muted/10 p-2 space-y-2">
        <Label className="text-sm font-semibold">
          {t("omo.advanced", { defaultValue: "Advanced Settings" })}
        </Label>
        {OMO_ADVANCED_JSON_FIELDS.filter(
          (field) => !isSlim || OMO_SLIM_ADVANCED_KEYS.has(field.key),
        ).map((field) => (
          <JsonTextareaField
            key={field.key}
            label={t(field.labelKey, { defaultValue: field.defaultLabel })}
            value={advancedFieldValues[field.key]}
            onChange={advancedFieldSetters[field.key]}
            placeholder={field.placeholder}
            minHeight={field.minHeight}
          />
        ))}

        <JsonTextareaField
          label={t("omo.otherFields", {
            defaultValue: "Other Config",
          })}
          value={otherFieldsStr}
          onChange={setOtherFieldsStr}
        />
      </div>
    </div>
  );
});
