import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProxyAppId } from "@/config/appConfig";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  useClaudeStackScenarios,
  useSetClaudeStackScenarios,
} from "@/lib/query/proxy";
import type { ClaudeStackScenarios } from "@/types/proxy";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

/** 下拉里的「跟随默认」：保存时转成 null（Radix 的选择项不能用空字符串做值）。 */
const FOLLOW = "__follow__";

const ROWS = [
  "haiku",
  "sonnet",
  "opus",
  "fable",
  "subagent",
  "auxiliary",
  "compaction",
] as const;

type Row = (typeof ROWS)[number];

/** 子代理和辅助 / 压缩请求没绑时跟随主模型；四档没绑时指向默认那家的第一个模型。 */
const FOLLOWS_MAIN: readonly Row[] = ["subagent", "auxiliary", "compaction"];

const normalize = (scenarios: ClaudeStackScenarios): ClaudeStackScenarios =>
  Object.fromEntries(ROWS.map((row) => [row, scenarios[row] || null]));

interface ClaudeScenarioSheetProps {
  app: ProxyAppId;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * 聚合（Stack）模式的「场景绑定」（#7889）：四档别名、子代理和辅助 / 压缩请求分别指向名单里
 * 已发布的模型；没绑的跟随默认行为。四档和子代理写进 settings.json（Claude Code 运行中就会
 * 重读），辅助 / 压缩由代理按请求类别分流。绑定失效（成员移除、行里删了模型）时投影按没绑
 * 处理，绑定值留着，成员加回来就恢复。
 */
export function ClaudeScenarioSheet({
  app,
  open,
  onOpenChange,
}: ClaudeScenarioSheetProps) {
  const { t } = useTranslation();
  const isClaude = app === "claude";
  // 打开时才查：绑定只在面板里用，别让每个供应商页都多一次取数。
  const { data } = useClaudeStackScenarios(app, isClaude && open);
  const save = useSetClaudeStackScenarios();
  // 打开时把绑定抄进草稿；打开期间重新拉到的数据不动草稿（别把正在改的覆盖掉）。
  const [draft, setDraft] = useState<ClaudeStackScenarios | null>(null);
  useEffect(() => {
    if (!open) {
      setDraft(null);
      return;
    }
    if (draft === null && data) {
      setDraft({ ...data.scenarios });
    }
  }, [open, data, draft]);

  if (!isClaude) {
    return null;
  }

  const models = data?.models ?? [];
  const dirty =
    draft !== null &&
    JSON.stringify(normalize(draft)) !==
      JSON.stringify(normalize(data?.scenarios ?? {}));

  const saveBindings = () => {
    if (!draft) return;
    save.mutate({ appType: app, scenarios: normalize(draft) });
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={420} closeLabel={t("common.close")}>
        <SheetHeader>
          <SheetTitle>{t("mode.scenarios.title")}</SheetTitle>
          <SheetDescription>
            {APP_DISPLAY_NAME[app]} · {t("mode.scenarios.description")}
          </SheetDescription>
        </SheetHeader>
        <SheetBody>
          {!draft ? (
            <p className="text-caption text-fg-2">{t("common.loading")}</p>
          ) : (
            <div className="flex flex-col gap-4">
              {models.length === 0 && (
                <p className="text-caption text-fg-2">
                  {t("mode.scenarios.empty")}
                </p>
              )}
              {ROWS.map((row) => {
                const bound = draft[row] || null;
                const stale =
                  bound !== null && !models.some((m) => m.id === bound);
                return (
                  <div key={row} className="flex flex-col gap-1">
                    <span className="text-caption text-fg-2">
                      {t(`mode.scenarios.${row}`)}
                    </span>
                    <Select
                      value={bound ?? FOLLOW}
                      disabled={models.length === 0 || save.isPending}
                      onValueChange={(value) =>
                        setDraft((current) =>
                          current === null
                            ? current
                            : {
                                ...current,
                                [row]: value === FOLLOW ? null : value,
                              },
                        )
                      }
                    >
                      <SelectTrigger
                        aria-label={t(`mode.scenarios.${row}`)}
                        className="text-body"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={FOLLOW}>
                          {FOLLOWS_MAIN.includes(row)
                            ? t("mode.scenarios.followMain")
                            : t("mode.scenarios.followDefault")}
                        </SelectItem>
                        {stale && bound && (
                          // 失效的绑定照原样显示（下面的提示解释），还能换成别的
                          <SelectItem value={bound} disabled>
                            {bound}
                          </SelectItem>
                        )}
                        {models.map((model) => (
                          <SelectItem key={model.id} value={model.id}>
                            {model.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {stale && (
                      <span className="text-caption text-warning-text">
                        {t("mode.scenarios.stale")}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </SheetBody>
        <SheetFooter>
          <Button
            variant="solid"
            size="compact"
            disabled={!draft || !dirty || save.isPending}
            onClick={saveBindings}
          >
            {t("common.save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
