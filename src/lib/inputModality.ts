/**
 * 记录用户最近用的是键盘还是指针：按 Tab / 方向键等「移动焦点」的键时给 <html> 加 data-keyboard
 * （Enter、空格、Esc 不算：在输入框里打字、回车不该让之后的点击带上焦点框），
 * 鼠标 / 触摸按下时去掉。index.css 只在 data-keyboard 下画焦点框，
 * 鼠标点按钮、页签不会留下一圈主题色描边（WebView 有时会对点击也触发 :focus-visible）。
 */
const NAVIGATION_KEYS = new Set([
  "Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

export function initializeInputModality() {
  const root = document.documentElement;
  window.addEventListener(
    "keydown",
    (event) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (NAVIGATION_KEYS.has(event.key)) root.dataset.keyboard = "";
    },
    true,
  );
  window.addEventListener(
    "pointerdown",
    () => {
      delete root.dataset.keyboard;
    },
    true,
  );
}

/** 最近一次操作是不是键盘（按过导航键、之后没再点鼠标） */
export function isKeyboardModality() {
  return document.documentElement.hasAttribute("data-keyboard");
}
