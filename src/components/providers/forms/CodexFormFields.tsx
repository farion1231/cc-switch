import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  ProviderCategory,
} from "@/types";

interface EndpointCandidate {
  url: string;
}

interface CodexFormFieldsProps {
  providerId?: string;
  // API Key
  codexApiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  shouldShowSpeedTest: boolean;
  codexBaseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isFullUrl: boolean;
  onFullUrlChange: (value: boolean) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // API Format
  // Note: wire_api is always "responses" for Codex; apiFormat controls proxy-layer conversion
  apiFormat: CodexApiFormat;
  onApiFormatChange: (format: CodexApiFormat) => void;
  codexChatReasoning?: CodexChatReasoning;
  onCodexChatReasoningChange?: (value: CodexChatReasoning) => void;

  // Model Catalog
  catalogModels?: CodexCatalogModel[];
  onCatalogModelsChange?: (models: CodexCatalogModel[]) => void;

  // Multimodal Fallback Model
  multimodalFallbackModel?: string;
  onMultimodalFallbackModelChange?: (model: string) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];
}

type CodexCatalogRow = CodexCatalogModel & { rowId: string };

function createCatalogRow(seed?: Partial<CodexCatalogModel>): CodexCatalogRow {
  return {
    rowId: crypto.randomUUID(),
    model: seed?.model ?? "",
    displayName: seed?.displayName ?? "",
    contextWindow: seed?.contextWindow ?? "",
  };
}

// Compares rows (with rowId) to incoming models (without) by data fields only,
// so both sync effects can use the same equality definition.
function catalogRowsMatchModels(
  rows: Array<Pick<CodexCatalogRow, "model" | "displayName" | "contextWindow">>,
  models: CodexCatalogModel[],
): boolean {
  if (rows.length !== models.length) return false;
  return rows.every((row, i) => {
    const incoming = models[i];
    return (
      row.model === (incoming.model ?? "") &&
      (row.displayName ?? "") === (incoming.displayName ?? "") &&
      String(row.contextWindow ?? "") === String(incoming.contextWindow ?? "")
    );
  });
}

export function CodexFormFields({
  providerId,
  codexApiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  shouldShowSpeedTest,
  codexBaseUrl,
  onBaseUrlChange,
  isFullUrl,
  onFullUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  apiFormat,
  onApiFormatChange,
  codexChatReasoning = {},
  onCodexChatReasoningChange,
  catalogModels = [],
  onCatalogModelsChange,
  multimodalFallbackModel = "",
  onMultimodalFallbackModelChange,
  speedTestEndpoints,
}: CodexFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [reasoningExpanded, setReasoningExpanded] = useState(false);
  const needsLocalRouting = apiFormat === "openai_chat";
  const canEditCatalog = Boolean(onCatalogModelsChange);
  const canEditReasoning = Boolean(onCodexChatReasoningChange);
  const supportsThinking =
    codexChatReasoning.supportsThinking === true ||
    codexChatReasoning.supportsEffort === true;
  const supportsEffort = codexChatReasoning.supportsEffort === true;

  const [catalogRows, setCatalogRows] = useState<CodexCatalogRow[]>(() =>
    catalogModels.map((m) => createCatalogRow(m)),
  );

  // 璁板綍涓婃鍙戦€佺粰鐖剁粍浠剁殑鏁版嵁锛岄伩鍏嶉噸澶嶈Е鍙?  const lastSentModelsRef = useRef<CodexCatalogModel[]>(catalogModels);

  // 鐖?鈫?瀛愶細浠呭綋 prop 鏁版嵁鐪熺殑鍙樺寲锛堥璁惧垏鎹?/ 缂栬緫鍔犺浇锛夋椂鎵嶉噸寤?rowId锛?  // 鍚?shape 鏃朵繚鐣欑幇鏈?rowId锛岄伩鍏嶇紪杈戣繃绋嬩腑鐒︾偣涓㈠け銆?  useEffect(() => {
    setCatalogRows((current) => {
      if (catalogRowsMatchModels(current, catalogModels)) return current;
      return catalogModels.map((m) => createCatalogRow(m));
    });
    // 鍚屾鏇存柊 ref锛岄伩鍏嶇埗缁勪欢浼犲叆鏂版暟鎹椂瀛愨啋鐖?effect 璇垽涓烘湰鍦颁慨鏀?    lastSentModelsRef.current = catalogModels;
  }, [catalogModels]);

  // 瀛?鈫?鐖讹細rowId 鏄鍥惧眰姒傚康锛屼笉搴旇繘鍏ユ寔涔呭寲鏁版嵁锛涘墺绂诲悗鍐嶅洖浼犮€?  // 娉ㄦ剰锛氫緷璧栨暟缁勪笉鍖呭惈 catalogModels锛岄伩鍏嶇埗鈫掑瓙鏇存柊瑙﹀彂瀛愨啋鐖跺洖璋冨舰鎴愬惊鐜€?  useEffect(() => {
    if (!onCatalogModelsChange) return;
    const next: CodexCatalogModel[] = catalogRows.map(
      ({ rowId: _rowId, ...rest }) => rest,
    );
    // 鍙湁褰撴暟鎹湡鐨勫彉鍖栨椂鎵嶉€氱煡鐖剁粍浠?    if (catalogRowsMatchModels(catalogRows, lastSentModelsRef.current)) return;
    lastSentModelsRef.current = next;
    onCatalogModelsChange(next);
  }, [catalogRows, onCatalogModelsChange]);

  const handleLocalRoutingChange = useCallback(
    (checked: boolean) => {
      onApiFormatChange(checked ? "openai_chat" : "openai_responses");
    },
    [onApiFormatChange],
  );

  const handleReasoningThinkingChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked,
        supportsEffort: checked ? codexChatReasoning.supportsEffort : false,
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleReasoningEffortChange = useCallback(
    (checked: boolean) => {
      if (!onCodexChatReasoningChange) return;
      onCodexChatReasoningChange({
        ...codexChatReasoning,
        supportsThinking: checked ? true : codexChatReasoning.supportsThinking,
        supportsEffort: checked,
        effortParam: checked
          ? (codexChatReasoning.effortParam ?? "reasoning_effort")
          : "none",
      });
    },
    [codexChatReasoning, onCodexChatReasoningChange],
  );

  const handleFetchModels = useCallback(() => {
    if (!codexBaseUrl || !codexApiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!codexApiKey,
        hasBaseUrl: !!codexBaseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(codexBaseUrl, codexApiKey, isFullUrl)
      .then((models) => {
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [codexBaseUrl, codexApiKey, isFullUrl, t]);

  const handleAddCatalogRow = useCallback(() => {
    if (!onCatalogModelsChange) return;
    setCatalogRows((current) => [...current, createCatalogRow()]);
  }, [onCatalogModelsChange]);

  const handleUpdateCatalogRow = useCallback(
    (index: number, patch: Partial<CodexCatalogModel>) => {
      setCatalogRows((current) =>
        current.map((row, i) => (i === index ? { ...row, ...patch } : row)),
      );
    },
    [],
  );

  const handleRemoveCatalogRow = useCallback((index: number) => {
    setCatalogRows((current) => current.filter((_, i) => i !== index));
  }, []);

  const renderCatalogActionButtons = (onAdd: () => void, addLabel: string) => (
    <div className="flex gap-1">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={handleFetchModels}
        disabled={isFetchingModels}
        className="h-7 gap-1"
      >
        {isFetchingModels ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Download className="h-3.5 w-3.5" />
        )}
        {t("providerForm.fetchModels")}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={onAdd}
        className="h-7 gap-1"
      >
        <Plus className="h-3.5 w-3.5" />
        {addLabel}
      </Button>
    </div>
  );

  return (
    <>
      {/* Codex API Key 杈撳叆妗?*/}
      <ApiKeySection
        id="codexApiKey"
        label="API Key"
        value={codexApiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
        placeholder={{
          official: t("providerForm.codexOfficialNoApiKey", {
            defaultValue: "瀹樻柟渚涘簲鍟嗘棤闇€ API Key",
          }),
          thirdParty: t("providerForm.codexApiKeyAutoFill", {
            defaultValue: "杈撳叆 API Key锛屽皢鑷姩濉厖鍒伴厤缃?,
          }),
        }}
      />

      {/* Codex Base URL 杈撳叆妗?*/}
      {shouldShowSpeedTest && (
        <EndpointField
          id="codexBaseUrl"
          label={t("codexConfig.apiUrlLabel")}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.codexApiEndpointPlaceholder")}
          hint={t("providerForm.codexApiHint")}
          showFullUrlToggle
          isFullUrl={isFullUrl}
          onFullUrlChange={onFullUrlChange}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {shouldShowSpeedTest && (
        <div className="space-y-3 rounded-lg border border-border-default bg-muted/20 p-4">
          <div className="flex items-center justify-between gap-4">
            <div className="space-y-1">
              <FormLabel>
                {t("codexConfig.localRoutingToggle", {
                  defaultValue: "闇€瑕佹湰鍦拌矾鐢辨槧灏?,
                })}
              </FormLabel>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {needsLocalRouting
                  ? t("codexConfig.localRoutingOnHint", {
                      defaultValue:
                        "Codex 鐩墠浠呭師鐢熸敮鎸?OpenAI Responses API 涓?GPT 绯诲垪妯″瀷锛涘鏋滄偍鐨勪緵搴斿晢浣跨敤 Chat Completions 鍗忚鎴栭潪 GPT 妯″瀷锛堝 DeepSeek銆並imi锛夛紝鍒欓渶瑕佹墦寮€鏈紑鍏筹紝骞跺湪浣跨敤杩囩▼涓繚鎸佹湰鍦拌矾鐢卞紑鍚€?,
                    })
                  : t("codexConfig.localRoutingOffHint", {
                      defaultValue:
                        "濡傛灉鎮ㄧ殑渚涘簲鍟嗕笉鏄師鐢?OpenAI Responses API锛屾垨鑰呮ā鍨嬪悕涓嶆槸 Codex 榛樿鐨?GPT 绯诲垪锛岃鎵撳紑姝ゅ紑鍏炽€?,
                    })}
              </p>
            </div>
            <Switch
              checked={needsLocalRouting}
              onCheckedChange={handleLocalRoutingChange}
              aria-label={t("codexConfig.localRoutingToggle", {
                defaultValue: "闇€瑕佹湰鍦拌矾鐢辨槧灏?,
              })}
            />
          </div>
        </div>
      )}

      {needsLocalRouting && canEditReasoning && (
        <Collapsible
          open={reasoningExpanded}
          onOpenChange={setReasoningExpanded}
          className="rounded-lg border border-border-default p-4"
        >
          <CollapsibleTrigger asChild>
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
            >
              {reasoningExpanded ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("codexConfig.reasoningSectionToggle", {
                defaultValue: "鎬濊€冭兘鍔涳紙楂樼骇路閫氬父鑷姩璇嗗埆锛?,
              })}
            </Button>
          </CollapsibleTrigger>
          {!reasoningExpanded && (
            <p className="mt-1 ml-1 text-xs text-muted-foreground">
              {t("codexConfig.reasoningSectionHint", {
                defaultValue:
                  "棰勮渚涘簲鍟嗗凡鑷姩閰嶇疆锛涜嚜瀹氫箟渚涘簲鍟嗕細鎸夊悕绉?鍦板潃鑷姩鎺ㄦ柇銆備粎褰撹嚜鍔ㄨ瘑鍒笉鍑嗘椂鎵嶉渶灞曞紑鎵嬪姩瑕嗙洊銆?,
              })}
            </p>
          )}
          <CollapsibleContent className="space-y-3 pt-3">
            <div className="flex items-center justify-between gap-4">
              <div className="space-y-1">
                <FormLabel>
                  {t("codexConfig.reasoningModeToggle", {
                    defaultValue: "鏀寔鎬濊€冩ā寮?,
                  })}
                </FormLabel>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {t("codexConfig.reasoningModeHint", {
                    defaultValue:
                      "涓婃父 Chat Completions 鎺ュ彛鏀寔寮€鍚垨鍏抽棴 thinking 鏃跺惎鐢ㄣ€侹imi銆丟LM銆丵wen 绛夐€氬父灞炰簬杩欎竴绫汇€?,
                  })}
                </p>
              </div>
              <Switch
                checked={supportsThinking}
                onCheckedChange={handleReasoningThinkingChange}
                aria-label={t("codexConfig.reasoningModeToggle", {
                  defaultValue: "鏀寔鎬濊€冩ā寮?,
                })}
              />
            </div>

            <div className="flex items-center justify-between gap-4 border-t border-border-default pt-3">
              <div className="space-y-1">
                <FormLabel>
                  {t("codexConfig.reasoningEffortToggle", {
                    defaultValue: "鏀寔鎬濊€冪瓑绾?,
                  })}
                </FormLabel>
                <p className="text-xs leading-relaxed text-muted-foreground">
                  {t("codexConfig.reasoningEffortHint", {
                    defaultValue:
                      "涓婃父鏀寔 low/high/max 绛夋€濊€冩繁搴︽帶鍒舵椂鍚敤銆傚惎鐢ㄥ悗浼氳嚜鍔ㄥ惎鐢ㄦ€濊€冩ā寮忥紝骞舵妸 Codex 鐨?reasoning.effort 杞垚涓婃父 Chat 鍙傛暟銆?,
                  })}
                </p>
              </div>
              <Switch
                checked={supportsEffort}
                onCheckedChange={handleReasoningEffortChange}
                aria-label={t("codexConfig.reasoningEffortToggle", {
                  defaultValue: "鏀寔鎬濊€冪瓑绾?,
                })}
              />
            </div>
          </CollapsibleContent>
        </Collapsible>
      )}

      {/* Codex 妯″瀷鏄犲皠 鈥斺€?浠呭湪鏈湴璺敱 + 鍙紪杈戞椂鏄剧ず */}
      {needsLocalRouting && canEditCatalog && (
        <div className="space-y-4 rounded-lg border border-border-default p-4">
          <div className="space-y-1">
            <div className="flex items-center justify-between gap-3">
              <FormLabel>
                {t("codexConfig.modelMappingTitle", {
                  defaultValue: "妯″瀷鏄犲皠",
                })}
              </FormLabel>
              {renderCatalogActionButtons(
                handleAddCatalogRow,
                t("codexConfig.addCatalogModel", {
                  defaultValue: "娣诲姞妯″瀷",
                }),
              )}
            </div>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t("codexConfig.modelMappingHint", {
                defaultValue:
                  "閫夋嫨妯″瀷瑙掕壊鍚庯紝CC Switch 浼氳嚜鍔ㄧ敓鎴?Codex 鍏煎璺敱锛涜彍鍗曟樉绀哄悕鍙互濉?DeepSeek銆並imi 绛夊搧鐗屾ā鍨嬶紝瀹為檯璇锋眰妯″瀷鎸夊彸渚у～鍐欏唴瀹瑰彂閫併€?,
              })}
            </p>
          </div>

          {catalogRows.length > 0 && (
            <div className="space-y-2">
              {/* 鍒楀ご锛歮d+ 鏄剧ず */}
              <div className="hidden grid-cols-[1fr_1fr_140px_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                <span>
                  {t("codexConfig.catalogColumnDisplay", {
                    defaultValue: "鑿滃崟鏄剧ず鍚?,
                  })}
                </span>
                <span>
                  {t("codexConfig.catalogColumnModel", {
                    defaultValue: "瀹為檯璇锋眰妯″瀷",
                  })}
                </span>
                <span>
                  {t("codexConfig.catalogColumnContext", {
                    defaultValue: "涓婁笅鏂囩獥鍙?,
                  })}
                </span>
                <span />
              </div>

              {catalogRows.map((row, index) => (
                <div
                  key={row.rowId}
                  className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_1fr_140px_36px]"
                >
                  <Input
                    value={row.displayName ?? ""}
                    onChange={(event) =>
                      handleUpdateCatalogRow(index, {
                        displayName: event.target.value,
                      })
                    }
                    placeholder={t(
                      "codexConfig.catalogDisplayNamePlaceholder",
                      {
                        defaultValue: "渚嬪: DeepSeek V4 Flash",
                      },
                    )}
                    aria-label={t("codexConfig.catalogColumnDisplay", {
                      defaultValue: "鑿滃崟鏄剧ず鍚?,
                    })}
                  />
                  <div className="flex gap-1">
                    <Input
                      value={row.model}
                      onChange={(event) =>
                        handleUpdateCatalogRow(index, {
                          model: event.target.value,
                        })
                      }
                      placeholder={t("codexConfig.catalogModelPlaceholder", {
                        defaultValue: "渚嬪: deepseek-v4-flash",
                      })}
                      aria-label={t("codexConfig.catalogColumnModel", {
                        defaultValue: "瀹為檯璇锋眰妯″瀷",
                      })}
                      className="flex-1"
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) =>
                          handleUpdateCatalogRow(index, {
                            model: id,
                            displayName: row.displayName?.trim()
                              ? row.displayName
                              : id,
                          })
                        }
                      />
                    )}
                  </div>
                  <Input
                    type="number"
                    min={1}
                    inputMode="numeric"
                    value={row.contextWindow ?? ""}
                    onChange={(event) =>
                      handleUpdateCatalogRow(index, {
                        contextWindow: event.target.value.replace(/[^\d]/g, ""),
                      })
                    }
                    placeholder={t("codexConfig.contextWindowPlaceholder", {
                      defaultValue: "渚嬪: 128000",
                    })}
                    aria-label={t("codexConfig.catalogColumnContext", {
                      defaultValue: "涓婁笅鏂囩獥鍙?,
                    })}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-9 w-9 text-muted-foreground hover:text-destructive"
                    onClick={() => handleRemoveCatalogRow(index)}
                    title={t("common.delete", { defaultValue: "鍒犻櫎" })}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* 澶氭ā鎬侀檷绾фā鍨?鈥斺€?浠呭湪鏈夊涓ā鍨嬫椂鏄剧ず */}
      {catalogModels.length > 1 && onMultimodalFallbackModelChange && (
        <div className="space-y-2 rounded-lg border border-border-default p-4">
          <FormLabel>
            {t("codexConfig.multimodalFallbackModel", {
              defaultValue: "澶氭ā鎬侀檷绾фā鍨?,
            })}
          </FormLabel>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t("codexConfig.multimodalFallbackHint", {
              defaultValue:
                "褰撹姹傚寘鍚浘鐗囦笖褰撳墠妯″瀷涓嶆敮鎸佸妯℃€佹椂锛岃嚜鍔ㄥ垏鎹㈠埌姝ゆā鍨嬨€傜暀绌哄垯涓嶉檷绾с€?,
            })}
          </p>
          <select
            className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            value={multimodalFallbackModel}
            onChange={(e) => onMultimodalFallbackModelChange(e.target.value)}
          >
            <option value="">
              {t("codexConfig.noFallback", { defaultValue: "涓嶉檷绾? })}
            </option>
            {catalogModels
              .filter((m) => {
                // 过滤掉当前主模型
                const mainModel = codexModel || currentModel;
                if (m.model === mainModel) return false;
                // 只显示支持多模态的模型（如果定义了 supportsMultimodal）
                if (m.supportsMultimodal === false) return false;
                return true;
              })
              .map((m) => (
              <option key={m.model} value={m.model}>
                {m.displayName || m.model}
              </option>
            ))}
          </select>
        </div>
      )}

      {/* 绔偣娴嬮€熷脊绐?- Codex */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="codex"
          providerId={providerId}
          value={codexBaseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}
    </>
  );
}
