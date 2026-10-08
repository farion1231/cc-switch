import { useCallback, useEffect, useRef } from "react";
import type { SyntheticEvent } from "react";

/**
 * 整页编辑器的「未保存改动」登记。
 *
 * 编辑器只盖住内容区，侧栏、托盘、⌘, 仍能离开当前页，离开会卸载编辑器、丢掉草稿。
 * 编辑器把用户动过表单登记在这里，外壳的导航入口离开前先问一句。
 * 保存成功或关闭后编辑器卸载，登记随之撤销。
 */
const dirtyEditors = new Set<object>();

export const hasUnsavedChanges = () => dirtyEditors.size > 0;

/** 用户确认放弃后清空登记，随后的导航不再重复询问 */
export const discardUnsavedChanges = () => dirtyEditors.clear();

/**
 * 这些控件被点一下就算改了：勾选、开关、单选、下拉选项、分段按钮。
 * 只切换显示的控件（显示密钥、表单 / JSON）放在 `data-unsaved-ignore` 里排除。
 */
const EDIT_CONTROL_SELECTOR = [
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="radio"]',
  '[role="option"]',
  '[role="menuitemcheckbox"]',
  '[role="menuitemradio"]',
  "[aria-pressed]",
].join(",");

/**
 * 挂到编辑器内容区：文字输入、原生选择变化、以及点到上面那些控件时登记为有未保存改动。
 * React 的合成事件沿组件树冒泡，渲染到 body 的下拉选项也能收到。
 * `enabled` 为 false 时不登记（例如新增供应商还在选预设）。
 */
export function useUnsavedChangesTracker(enabled = true) {
  const token = useRef({}).current;
  useEffect(() => {
    if (!enabled) dirtyEditors.delete(token);
    return () => {
      dirtyEditors.delete(token);
    };
  }, [enabled, token]);

  const markDirty = useCallback(() => {
    if (enabled) dirtyEditors.add(token);
  }, [enabled, token]);

  const onClickCapture = useCallback(
    (event: SyntheticEvent) => {
      const target = event.target as Element | null;
      if (
        target?.closest?.(EDIT_CONTROL_SELECTOR) &&
        !target.closest("[data-unsaved-ignore]")
      ) {
        markDirty();
      }
    },
    [markDirty],
  );

  return {
    onInputCapture: markDirty,
    onChangeCapture: markDirty,
    onClickCapture,
  };
}
