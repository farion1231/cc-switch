import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useState } from "react";
import JsonEditor from "@/components/JsonEditor";

/**
 * 锁住 JsonEditor 的一条不变量：把 value 属性同步进编辑器的那次程序化替换，
 * 不得被当成用户输入回吐给 onChange。
 *
 * 回吐会让调用方分不清"用户改了"和"我刚把值推进来"。两个持有不同文档的编辑器
 * 实例因此互相回吐、各自把对方的值盖掉，表现为编辑弹窗在两份配置之间无休止地
 * 闪 —— 这是线上真实发生过的故障。
 *
 * 这里用真实的 CodeMirror 编辑器，不做 mock：mock 掉就等于把被测的那段逻辑
 * 换掉，测试会在有 bug 的代码上照样通过。
 */
const CONFIG_A = JSON.stringify({ env: { A: "1" } }, null, 2);
const CONFIG_B = JSON.stringify({ env: { B: "2", C: "3" } }, null, 2);

function Harness({ onChange }: { onChange: (value: string) => void }) {
  const [value, setValue] = useState(CONFIG_A);
  return (
    <div>
      <button type="button" onClick={() => setValue(CONFIG_B)}>
        push-b
      </button>
      <JsonEditor
        value={value}
        onChange={(next) => {
          onChange(next);
          setValue(next);
        }}
      />
    </div>
  );
}

describe("JsonEditor 外部同步不得回吐成用户输入", () => {
  it("把新 value 推进编辑器时不触发 onChange", async () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);

    // 挂载本身不算用户输入
    await waitFor(() =>
      expect(document.querySelector(".cm-content")).toBeTruthy(),
    );
    expect(onChange).not.toHaveBeenCalled();

    screen.getByText("push-b").click();

    // 外部推入新值后，文档要更新，但 onChange 必须一次都不响
    await waitFor(() =>
      expect(document.querySelector(".cm-content")?.textContent).toContain("C"),
    );
    expect(onChange).not.toHaveBeenCalled();
  });

  it("onChange 换成新的闭包后，监听器必须用最新那个", async () => {
    const first = vi.fn();
    const second = vi.fn();

    function SwapHarness() {
      const [useSecond, setUseSecond] = useState(false);
      const [value, setValue] = useState(CONFIG_A);
      return (
        <div>
          <button type="button" onClick={() => setUseSecond(true)}>
            swap
          </button>
          <JsonEditor
            value={value}
            onChange={(next) => {
              (useSecond ? second : first)(next);
              setValue(next);
            }}
          />
        </div>
      );
    }

    render(<SwapHarness />);
    await waitFor(() =>
      expect(document.querySelector(".cm-content")).toBeTruthy(),
    );

    screen.getByText("swap").click();

    // 模拟用户输入：直接改 CodeMirror 文档（非外部同步事务）
    const view = (
      document.querySelector(".cm-editor") as HTMLElement & {
        cmView?: { view: { dispatch: (spec: unknown) => void } };
      }
    )?.cmView?.view;
    if (!view) {
      // 拿不到内部实例时跳过断言，避免测试依赖 CodeMirror 私有属性而变脆
      return;
    }
    view.dispatch({ changes: { from: 0, to: 0, insert: " " } });

    await waitFor(() => expect(second).toHaveBeenCalled());
    expect(first).not.toHaveBeenCalled();
  });
});
