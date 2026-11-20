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
  "[data-unsaved-edit]",
].join(",");

/**
 * 嵌套的编辑器（例如供应商表单里打开的子代理编辑器）各自登记自己的改动：一个事件只归
 * 离它最近的那层。按 React 组件树而不是 DOM 判断，portal 出去的编辑器和下拉选项也
 * 算在所属的那层里。
 *
 * 捕获阶段由外到内执行：每层第一次见到这个原生事件时排进 chain，更内层的到来时把
 * 上一层这次刚登记的撤销掉（它之前本来就有改动的不撤）。同一原生事件还会派发成别的
 * 合成事件（input 之后还有 change），这时只有 chain 最后那层（最内层）算数。
 */
const eventClaims = new WeakMap<
  Event,
  { chain: object[]; ownerWasDirty: boolean }
>();

/** 返回这一层是不是这个事件的归属（最内层） */
function claimEvent(event: SyntheticEvent, token: object): boolean {
  const native = event.nativeEvent;
  const claim = eventClaims.get(native);
  if (!claim) {
    eventClaims.set(native, {
      chain: [token],
      ownerWasDirty: dirtyEditors.has(token),
    });
    return true;
  }
  const owner = claim.chain[claim.chain.length - 1];
  if (owner === token) return true;
  if (claim.chain.includes(token)) return false;
  if (!claim.ownerWasDirty) dirtyEditors.delete(owner);
  claim.chain.push(token);
  claim.ownerWasDirty = dirtyEditors.has(token);
  return true;
}

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

  const onInputOrChangeCapture = useCallback(
    (event: SyntheticEvent) => {
      if (!enabled || !claimEvent(event, token)) return;
      const target = event.target as Element | null;
      if (!target?.closest?.("[data-unsaved-ignore]")) {
        markDirty();
      }
    },
    [enabled, markDirty, token],
  );

  const onClickCapture = useCallback(
    (event: SyntheticEvent) => {
      if (!enabled || !claimEvent(event, token)) return;
      const target = event.target as Element | null;
      if (
        target?.closest?.(EDIT_CONTROL_SELECTOR) &&
        !target.closest("[data-unsaved-ignore]")
      ) {
        markDirty();
      }
    },
    [enabled, markDirty, token],
  );

  return {
    onInputCapture: onInputOrChangeCapture,
    onChangeCapture: onInputOrChangeCapture,
    onClickCapture,
  };
}
