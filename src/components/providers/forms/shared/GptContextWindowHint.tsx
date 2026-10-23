import { useTranslation } from "react-i18next";

import {
  GPT_CONTEXT_WINDOW_CAP,
  isGptModelId,
  positiveInteger,
} from "@/lib/modelMetadata";
import { cn } from "@/lib/utils";

/**
 * GPT 模型的窗口填得比订阅窗口大时，在输入框下提醒一句。手填的值照用不拦，
 * 只是中转站的 GPT 多半来自订阅，写大了要等上游拒收才失败。
 */
export function GptContextWindowHint({
  modelId,
  contextWindow,
  className,
}: {
  modelId: string | undefined;
  contextWindow: unknown;
  className?: string;
}) {
  const { t } = useTranslation();
  const window = positiveInteger(contextWindow);
  if (
    !modelId ||
    !isGptModelId(modelId) ||
    window === undefined ||
    window <= GPT_CONTEXT_WINDOW_CAP
  ) {
    return null;
  }
  return (
    <p className={cn("text-xs leading-relaxed text-warning-text", className)}>
      {t("providerForm.gptContextWindowHint", {
        defaultValue:
          "中转站的 GPT 多来自 ChatGPT 订阅，窗口只有 272K。窗口写大了不会提前压缩，等上游拒收时这一轮会失败；确认这家支持更大窗口再这样填。",
      })}
    </p>
  );
}
