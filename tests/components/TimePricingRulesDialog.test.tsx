import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
  cleanup,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { TimePricingRulesDialog } from "@/components/usage/TimePricingRulesDialog";
import type { ModelPricing, ModelPricingTimeRule } from "@/types/usage";

const mutateAsync = vi.fn(async (rules: ModelPricingTimeRule[]) => rules);

/** 够用的迷你 i18next：有 defaultValue 就用它，再把 {{var}} 插进去 */
const translate = (key: string, options?: string | Record<string, unknown>) => {
  if (typeof options === "string") return options;
  let text = (options?.defaultValue as string | undefined) ?? key;
  for (const [name, value] of Object.entries(options ?? {})) {
    if (name === "defaultValue") continue;
    text = text.split(`{{${name}}}`).join(String(value));
  }
  return text;
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: translate }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/lib/query/usage", () => ({
  useModelPricingTimeRules: () => ({
    data: currentRules,
    isLoading: false,
    error: null,
    refetch: vi.fn(),
  }),
  useModelPricing: () => ({ data: knownPricing }),
  useSaveModelPricingTimeRules: () => ({ mutateAsync, isPending: false }),
}));

// mock 工厂在引用声明之前跑，所以用 var 形式的提升拿到这两个变量
var currentRules: ModelPricingTimeRule[] = [];
var knownPricing: ModelPricing[] = [];

const peak: ModelPricingTimeRule = {
  id: "peak-1",
  modelId: "deepseek-chat",
  label: "Peak hours",
  utcOffsetMinutes: 0,
  weekdayMask: 31,
  startMinute: 60,
  endMinute: 240,
  excludedDates: ["2026-10-01"],
  inputCostPerMillion: "0.3",
  outputCostPerMillion: "1.2",
  cacheReadCostPerMillion: "0.006",
  cacheCreationCostPerMillion: "0",
};

const offPeak: ModelPricingTimeRule = {
  id: "off-peak",
  modelId: "deepseek-chat",
  label: "Off-peak",
  utcOffsetMinutes: 0,
  weekdayMask: 127,
  startMinute: 0,
  endMinute: 0,
  excludedDates: [],
  inputCostPerMillion: "0.15",
  outputCostPerMillion: "0.6",
  cacheReadCostPerMillion: "0.003",
  cacheCreationCostPerMillion: "0",
};

const PEAK_SUMMARY =
  "Peak hours · deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00) · except 2026-10-01";
const OFF_PEAK_SUMMARY =
  "Off-peak · deepseek-chat · Every day all day (UTC+00:00)";

/**
 * Radix 把对话框直接 portal 进 document.body，而 `screen` 绑定的是整个 body——
 * 也就是说，只要 body 里还留着别处渲染过的节点，下面每一条查询都可能撞上它。
 * 所以这里做两件事，让查询在结构上就不可能匹配到第二个元素：
 *
 * 1. 渲染前先把 body 变成我们自己的（setupTests 的 afterEach 已经 cleanup 过，
 *    这里再兜一次底，任何残留都活不到查询时刻）。
 * 2. 把每条查询都收口到本次渲染出来的那个对话框上。`getByRole` 默认忽略
 *    aria-hidden 子树，而 Radix 的 hideOthers 会把活动对话框之外的节点标成
 *    aria-hidden，所以这个作用域永远只指向一个对话框。
 */
function renderDialog() {
  render(<TimePricingRulesDialog open onClose={() => {}} />);
  return within(screen.getByRole("dialog"));
}

/** 同一父元素下，a 是否真的排在 b 前面 */
function isBefore(a: HTMLElement, b: HTMLElement) {
  return Boolean(
    a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING,
  );
}

beforeEach(() => {
  // 兜底清场：cleanup() 之后 body 本该是空的，真有残留也活不过这一行，
  // 后面的查询因此只可能命中本用例自己渲染出来的那一个对话框。
  cleanup();
  document.body.replaceChildren();

  currentRules = [peak, offPeak];
  knownPricing = [
    {
      modelId: "deepseek-chat",
      displayName: "DeepSeek Chat",
      inputCostPerMillion: "0.15",
      outputCostPerMillion: "0.6",
      cacheReadCostPerMillion: "0.003",
      cacheCreationCostPerMillion: "0",
    },
  ];
  mutateAsync.mockClear();
});

describe("TimePricingRulesDialog", () => {
  it("renders the server's rules in the server's order", () => {
    const dialog = renderDialog();

    const first = dialog.getByText(PEAK_SUMMARY);
    const second = dialog.getByText(OFF_PEAK_SUMMARY);
    expect(isBefore(first, second)).toBe(true);
  });

  it("explains that a missing rule falls back to the single-tier price", () => {
    const dialog = renderDialog();
    expect(
      dialog.getByText(
        "The model's ordinary single-tier price is used, unchanged. A rule only overrides the price inside its own window.",
      ),
    ).toBeInTheDocument();
  });

  it("shows an empty state instead of a blank list", () => {
    currentRules = [];
    const dialog = renderDialog();
    expect(
      dialog.getByText(
        "No time-based rules yet. Every model keeps its single-tier price until you add one.",
      ),
    ).toBeInTheDocument();
  });

  it("sends the reordered list when a rule is moved up", async () => {
    const dialog = renderDialog();

    const moveUpButtons = dialog.getAllByLabelText("Move up");
    // 第一条的「上移」是禁用的，能动的是第二条
    expect(moveUpButtons[0]).toBeDisabled();
    fireEvent.click(moveUpButtons[1]);

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync.mock.calls[0][0].map((rule) => rule.id)).toEqual([
      "off-peak",
      "peak-1",
    ]);
    await waitFor(() =>
      expect(
        isBefore(
          dialog.getByText(OFF_PEAK_SUMMARY),
          dialog.getByText(PEAK_SUMMARY),
        ),
      ).toBe(true),
    );
  });

  it("sends the reordered list when a rule is moved down", async () => {
    const dialog = renderDialog();

    const moveDownButtons = dialog.getAllByLabelText("Move down");
    expect(moveDownButtons[0]).toBeEnabled();
    expect(moveDownButtons[1]).toBeDisabled();
    fireEvent.click(moveDownButtons[0]);

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync.mock.calls[0][0].map((rule) => rule.id)).toEqual([
      "off-peak",
      "peak-1",
    ]);
  });

  it("offers the configured model ids for the model field", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    const modelInput = dialog.getByLabelText("Model ID") as HTMLInputElement;
    expect(modelInput.getAttribute("list")).toBe("time-rule-model-options");
    expect(
      document
        .getElementById("time-rule-model-options")
        ?.querySelectorAll("option"),
    ).toHaveLength(1);
  });

  it("renders the UTC offset presets and applies one on click", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    const offsetInput = dialog.getByLabelText(
      "UTC offset (minutes)",
    ) as HTMLInputElement;
    expect(offsetInput.value).toBe("0");
    expect(dialog.getByText("UTC+00:00")).toBeInTheDocument();

    fireEvent.click(dialog.getByText("UTC+08:00 · Asia/Shanghai"));
    expect(offsetInput.value).toBe("480");
    expect(dialog.getByText("UTC+08:00")).toBeInTheDocument();
  });

  it("blocks an empty model id and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));
    fireEvent.click(dialog.getByText("Save rule"));

    expect(await dialog.findByText("Pick a model ID")).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks an empty weekday mask and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    for (const checkbox of dialog.getAllByRole("checkbox")) {
      fireEvent.click(checkbox);
    }
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("Pick at least one day"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks an out-of-range start minute and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Start"), {
      target: { value: "24:00" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("Start must be HH:MM between 00:00 and 23:59"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks an out-of-range end minute and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("End"), {
      target: { value: "12:60" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("End must be HH:MM between 00:00 and 24:00"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks a malformed excluded date and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Excluded dates"), {
      target: { value: "2026/10/01" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("Dates must look like YYYY-MM-DD"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks a date that does not exist and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Excluded dates"), {
      target: { value: "2026-02-30" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("That date does not exist"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks a negative price and shows the reason", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Input cost (per 1M tokens, USD)"), {
      target: { value: "-1" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText("Prices must be non-negative decimals"),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("blocks a blank price instead of silently treating it as 0", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Model ID"), {
      target: { value: "deepseek-chat" },
    });
    // 四个单价都故意留空：新建规则不预填 0
    fireEvent.click(dialog.getByText("Save rule"));

    expect(
      await dialog.findByText(
        "Fill in all four prices — a blank one is billed as nothing",
      ),
    ).toBeInTheDocument();
    expect(mutateAsync).not.toHaveBeenCalled();
  });

  it("explains the cache write assumption on the field itself", () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    // 供应商很少单独公布缓存写入价；字段上要直接说明「照抄输入价、别留 0」
    expect(
      dialog.getByText(/Many providers charge a cache write at the same rate/),
    ).toBeInTheDocument();
    expect(
      dialog.getByText(/0 bills every cache write as nothing/),
    ).toBeInTheDocument();
  });

  it("summarises a new rule live before it is saved", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getByText("Add rule"));

    fireEvent.change(dialog.getByLabelText("Model ID"), {
      target: { value: "deepseek-chat" },
    });
    fireEvent.change(dialog.getByLabelText("Label (optional)"), {
      target: { value: "Peak hours" },
    });
    fireEvent.click(dialog.getByText("Weekdays"));
    fireEvent.change(dialog.getByLabelText("Start"), {
      target: { value: "22:00" },
    });
    fireEvent.change(dialog.getByLabelText("End"), {
      target: { value: "08:00" },
    });
    fireEvent.change(dialog.getByLabelText("Excluded dates"), {
      target: { value: "2026-10-01" },
    });
    // DeepSeek 不单独公布缓存写入价：照抄同一档的输入价
    fireEvent.change(dialog.getByLabelText("Input cost (per 1M tokens, USD)"), {
      target: { value: "0.30" },
    });
    fireEvent.change(
      dialog.getByLabelText("Output cost (per 1M tokens, USD)"),
      { target: { value: "1.20" } },
    );
    fireEvent.change(
      dialog.getByLabelText("Cache hit cost (per 1M tokens, USD)"),
      { target: { value: "0.006" } },
    );
    fireEvent.change(
      dialog.getByLabelText("Cache write cost (per 1M tokens, USD)"),
      { target: { value: "0.30" } },
    );

    expect(
      dialog.getByText(
        "Peak hours · deepseek-chat · Mon–Fri 22:00–08:00 (UTC+00:00) · except 2026-10-01",
      ),
    ).toBeInTheDocument();

    fireEvent.click(dialog.getByText("Save rule"));
    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    const saved = mutateAsync.mock.calls[0][0];
    expect(saved).toHaveLength(3);
    expect(saved[2]).toMatchObject({
      modelId: "deepseek-chat",
      label: "Peak hours",
      utcOffsetMinutes: 0,
      weekdayMask: 31,
      startMinute: 1320,
      endMinute: 480,
      excludedDates: ["2026-10-01"],
      // 按用户敲的原样存（和 PricingEditModal 一致），不做数值归一
      inputCostPerMillion: "0.30",
      outputCostPerMillion: "1.20",
      cacheReadCostPerMillion: "0.006",
      cacheCreationCostPerMillion: "0.30",
    });
  });

  it("edits an existing rule in place and keeps its position", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getAllByLabelText("Edit rule 1")[0]);

    fireEvent.change(dialog.getByLabelText("Start"), {
      target: { value: "02:00" },
    });
    fireEvent.click(dialog.getByText("Save rule"));

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    const saved = mutateAsync.mock.calls[0][0];
    expect(saved.map((rule) => rule.id)).toEqual(["peak-1", "off-peak"]);
    expect(saved[0].startMinute).toBe(120);
  });

  it("deletes a rule after confirmation", async () => {
    const dialog = renderDialog();
    fireEvent.click(dialog.getAllByLabelText("Delete rule 2")[0]);

    fireEvent.click(screen.getByText("Delete"));

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledTimes(1));
    expect(mutateAsync.mock.calls[0][0].map((rule) => rule.id)).toEqual([
      "peak-1",
    ]);
  });

  it("separates rules by model id", () => {
    currentRules = [
      offPeak,
      { ...peak, id: "other", modelId: "gpt-4o-mini", label: "" },
    ];
    const dialog = renderDialog();

    // 组标题按 modelId 分开，不再是一条接一条看不出属于谁
    expect(dialog.getByText("gpt-4o-mini")).toBeInTheDocument();
    expect(
      dialog.getByText(
        "Off-peak · deepseek-chat · Every day all day (UTC+00:00)",
      ),
    ).toBeInTheDocument();
    expect(
      dialog.getByText(
        "gpt-4o-mini · Mon–Fri 01:00–04:00 (UTC+00:00) · except 2026-10-01",
      ),
    ).toBeInTheDocument();
  });

  it("shows each rule's four prices on one line", () => {
    const dialog = renderDialog();
    expect(
      dialog.getByText(
        "in $0.3 · out $1.2 · cache hit $0.006 · cache write $0",
      ),
    ).toBeInTheDocument();
  });
});
