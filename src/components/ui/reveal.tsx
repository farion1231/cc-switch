import * as React from "react";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";

// 和折叠区、通知槽同一条减速曲线
const REVEAL_TRANSITION = { duration: 0.22, ease: [0.2, 0, 0, 1] } as const;

/**
 * 跟着开关出现 / 消失的一块（「开机自启」打开后的「静默启动」、开启保护后的阈值……）：
 * 高度跟着动，下面的行是被慢慢推开、慢慢收回的，不会一下跳过去。
 * - 页面第一次画出来时已经是打开的就不播（initial={false}），只在切换时动；
 * - 裁切只在动画期间加：展开完就撤掉，不裁里面输入框的焦点环；
 * - 放在 SettingsCard（divide-y）里时，这一层就是那一行，分隔线跟着它一起出现、收起。
 */
export function Reveal({
  show,
  children,
  className,
}: {
  show: boolean;
  children: React.ReactNode;
  className?: string;
}) {
  const reduceMotion = useReducedMotion();
  return (
    <AnimatePresence initial={false}>
      {show && (
        <motion.div
          className={className}
          initial={{ height: 0, opacity: 0, overflow: "hidden" }}
          animate={{
            height: "auto",
            opacity: 1,
            transitionEnd: { overflow: "visible" },
          }}
          exit={{ height: 0, opacity: 0, overflow: "hidden" }}
          transition={reduceMotion ? { duration: 0 } : REVEAL_TRANSITION}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
