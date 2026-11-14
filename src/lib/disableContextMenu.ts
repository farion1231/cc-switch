/**
 * 可编辑区域与可复制内容保留原生右键菜单（复制、粘贴、拼写检查等）。
 * 与 src/index.css 中的 user-select 白名单保持同步。
 */
const EDITABLE_SELECTOR =
  "input, textarea, select, [contenteditable='true'], [contenteditable=''], .cm-editor, pre, code, .selectable";

/**
 * 禁用 WebView 默认的浏览器右键菜单（后退、刷新、另存为、打印等）。
 * 只读文本被选中时仍保留原生菜单，方便鼠标右键复制。
 */
export function disableContextMenu(): void {
  if (typeof window === "undefined") return;

  window.addEventListener("contextmenu", (event) => {
    const target = event.target;
    if (target instanceof Element && target.closest(EDITABLE_SELECTOR)) {
      return;
    }

    if (window.getSelection()?.toString()) {
      return;
    }

    event.preventDefault();
  });
}
