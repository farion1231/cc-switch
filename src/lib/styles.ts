/**
 * 复用的 Tailwind 样式组合，覆盖常见 UI 模式
 */

// 按钮样式
export const buttonStyles = {
  // 主按钮：蓝底白字
  primary:
    "px-4 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 transition-colors text-sm font-medium",

  // 次按钮：灰背景，深色文本
  secondary:
    "px-4 py-2 text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800 dark:hover:text-gray-200 rounded-lg transition-colors text-sm font-medium",

  // 危险按钮：用于不可撤销/破坏性操作
  danger:
    "px-4 py-2 bg-red-500 text-white rounded-lg hover:bg-red-600 dark:bg-red-600 dark:hover:bg-red-700 transition-colors text-sm font-medium",

  // 幽灵按钮：无背景，仅悬浮反馈
  ghost:
    "px-4 py-2 text-gray-500 hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors text-sm font-medium",

  // 图标按钮：小尺寸，仅图标
  icon: "p-1.5 text-gray-500 hover:text-gray-900 hover:bg-gray-100 dark:text-gray-400 dark:hover:text-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors",

  // 禁用态：可与其他样式组合
  disabled: "opacity-50 cursor-not-allowed pointer-events-none",
} as const;

// 卡片样式
export const cardStyles = {
  // 基础卡片容器
  base: "bg-white rounded-lg border border-gray-200 p-4 dark:bg-gray-900 dark:border-gray-700",

  // 带悬浮效果的卡片
  interactive:
    "bg-white rounded-lg border border-gray-200 p-4 hover:border-gray-300 hover:shadow-sm dark:bg-gray-900 dark:border-gray-700 dark:hover:border-gray-600 transition-all duration-200",

  // 选中/激活态卡片
  selected:
    "bg-white rounded-lg border border-blue-500 shadow-sm bg-blue-50 p-4 dark:bg-gray-900 dark:border-blue-400 dark:bg-blue-400/10",
} as const;

// 输入控件样式
export const inputStyles = {
  // 文本输入框
  text: "w-full px-3 py-2 border border-gray-200 rounded-lg focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 outline-none dark:bg-gray-900 dark:border-gray-700 dark:text-gray-100 dark:focus:border-blue-400 dark:focus:ring-blue-400/20 transition-colors",

  // 下拉选择框
  select:
    "w-full px-3 py-2 border border-gray-200 rounded-lg focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 outline-none bg-white dark:bg-gray-900 dark:border-gray-700 dark:text-gray-100 dark:focus:border-blue-400 dark:focus:ring-blue-400/20 transition-colors",

  // 复选框
  checkbox:
    "w-4 h-4 text-blue-500 rounded focus:ring-blue-500/20 border-gray-300 dark:border-gray-600 dark:bg-gray-800",
} as const;

// 徽标（Badge）样式
export const badgeStyles = {
  // 成功徽标
  success:
    "inline-flex items-center gap-1 px-2 py-1 bg-green-500/10 text-green-500 rounded-md text-xs font-medium",

  // 信息徽标
  info: "inline-flex items-center gap-1 px-2 py-1 bg-blue-500/10 text-blue-500 rounded-md text-xs font-medium",

  // 警告徽标
  warning:
    "inline-flex items-center gap-1 px-2 py-1 bg-amber-500/10 text-amber-500 rounded-md text-xs font-medium",

  // 错误徽标
  error:
    "inline-flex items-center gap-1 px-2 py-1 bg-red-500/10 text-red-500 rounded-md text-xs font-medium",
} as const;

// 组合类名的工具函数
export function cn(...classes: (string | undefined | false)[]) {
  return classes.filter(Boolean).join(" ");
}
