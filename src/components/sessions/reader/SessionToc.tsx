import { memo } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { TurnIndex } from "@/types";
import { formatMessageTime } from "../utils";
import { useReaderT } from "./i18n";

interface SessionQuestionsMenuProps {
  /** 轮次索引（后端 Header.turns；旧后端由前端 buildTurnIndex 兜底），已排除注入轮 */
  items: TurnIndex[];
  onItemClick: (item: TurnIndex) => void;
}

/**
 * 阅读页的「提问 12 ⌄」：每轮一项（序号 + 提问预览 + 时间），失败 / 中断的轮在右侧标出，
 * 点一项滚到那一轮。
 */
export const SessionQuestionsMenu = memo(function SessionQuestionsMenu({
  items,
  onItemClick,
}: SessionQuestionsMenuProps) {
  const { t } = useTranslation();
  const rt = useReaderT();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="quiet"
          size="regular"
          disabled={items.length === 0}
          className="shrink-0 gap-1 pe-2 ps-2.5 font-normal text-fg-2"
        >
          {t("sessionManager.questions", { defaultValue: "提问" })}
          <span className="tabular-nums">{items.length}</span>
          <ChevronDown className="h-3.5 w-3.5" strokeWidth={2} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        aria-label={t("sessionManager.questionsMenu", {
          defaultValue: "跳到提问",
        })}
        className="max-h-[320px] w-[400px] max-w-[calc(100vw-32px)] overflow-y-auto overscroll-contain rounded-panel p-1 shadow-v7-md"
      >
        {items.map((item, index) => (
          <DropdownMenuItem
            key={`${item.turnId}:${item.firstMessageIndex}`}
            title={item.questionPreview}
            onSelect={() => onItemClick(item)}
            className="h-8 gap-2.5 rounded-control px-2.5 text-body"
          >
            <span className="w-[18px] shrink-0 text-right text-caption tabular-nums text-fg-3">
              {index + 1}
            </span>
            <span className="min-w-0 flex-1 truncate">
              {item.questionPreview}
            </span>
            {item.errorCount > 0 && (
              <span className="shrink-0 text-caption tabular-nums text-danger-text">
                {rt("summaryErrors", { count: item.errorCount })}
              </span>
            )}
            {item.aborted && (
              <span className="shrink-0 text-caption text-warning-text">
                {rt("turnAborted")}
              </span>
            )}
            {item.ts ? (
              <span className="shrink-0 text-caption tabular-nums text-fg-3">
                {formatMessageTime(item.ts)}
              </span>
            ) : null}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
});
