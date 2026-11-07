import { useLayoutEffect, useRef, useState, type RefObject } from "react";

/** 名字列至少留这么宽；再窄，主操作按钮就收成纯图标 */
const MIN_NAME_WIDTH = 96;
/** 放回文字时多留的余量，免得在临界宽度上来回跳 */
const RESTORE_SLACK = 8;

/**
 * 主操作按钮带名字时，窄窗口、大缩放下会把名字列挤没，⋯ 被挤出卡片后让列表的 overflow-x-hidden 裁掉。
 * 这里量名字列的实际宽度：窄于 MIN_NAME_WIDTH 就收成纯图标；收起后名字列会多出按钮文字让出的那段，
 * 要等扣掉这段还够宽才放回文字。
 * 按实际宽度而不是窗口宽度判断：按钮文字随语言变，额度列、上下移动、终端按钮也不是每张卡都有。
 *
 * resetKey 是按钮文字：文字变了（换语言、换模式）就回到带文字重新量。
 */
export function useIconOnlyActions(
  nameRef: RefObject<HTMLElement>,
  resetKey: string,
): boolean {
  const [iconOnly, setIconOnly] = useState(false);
  const iconOnlyRef = useRef(false);
  /** 收成图标时名字列的宽度；收起后的那次布局拿它算出让出了多少 */
  const widthBeforeRef = useRef<number | null>(null);
  const freedRef = useRef(0);

  const update = (next: boolean) => {
    iconOnlyRef.current = next;
    setIconOnly(next);
  };

  useLayoutEffect(() => {
    if (iconOnlyRef.current) update(false);
  }, [resetKey]);

  useLayoutEffect(() => {
    const name = nameRef.current;
    if (!iconOnly || !name || widthBeforeRef.current === null) return;
    freedRef.current = Math.max(
      0,
      name.getBoundingClientRect().width - widthBeforeRef.current,
    );
    widthBeforeRef.current = null;
  }, [iconOnly, nameRef]);

  useLayoutEffect(() => {
    const name = nameRef.current;
    if (!name || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      const width = name.getBoundingClientRect().width;
      if (!iconOnlyRef.current) {
        if (width < MIN_NAME_WIDTH) {
          widthBeforeRef.current = width;
          update(true);
        }
      } else if (
        widthBeforeRef.current === null &&
        width - freedRef.current >= MIN_NAME_WIDTH + RESTORE_SLACK
      ) {
        update(false);
      }
    });
    observer.observe(name);
    return () => observer.disconnect();
  }, [nameRef]);

  return iconOnly;
}
