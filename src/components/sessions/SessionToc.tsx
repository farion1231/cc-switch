import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

export interface SessionTocItem {
  /** 在原始消息数组里的下标 */
  index: number;
  preview: string;
  ts?: number;
}

interface SessionQuestionsMenuProps {
  items: SessionTocItem[];
  onItemClick: (index: number) => void;
}

/** 阅读页的「提问 12 ⌄」：列出用户提问（序号 + 前 40 字），点一项滚到那条消息。 */
export function SessionQuestionsMenu({
  items,
  onItemClick,
}: SessionQuestionsMenuProps) {
  const { t } = useTranslation();

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
        className="max-h-[300px] w-[380px] max-w-[calc(100vw-32px)] overflow-y-auto overscroll-contain rounded-panel p-1 shadow-v7-md"
      >
        {items.map((item, tocIndex) => (
          <DropdownMenuItem
            key={item.index}
            title={item.preview}
            onSelect={() => onItemClick(item.index)}
            className="h-8 gap-2.5 rounded-control px-2.5 text-body"
          >
            <span className="w-[18px] shrink-0 text-right text-caption tabular-nums text-fg-3">
              {tocIndex + 1}
            </span>
            <span className="min-w-0 truncate">{item.preview}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
