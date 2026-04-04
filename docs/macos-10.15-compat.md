# macOS 10.15 (Catalina) 兼容性适配记录

## 背景

CC Switch 原本 `minimumSystemVersion` 为 `12.0`，需要降低到 `10.15` 以支持 macOS Catalina。

macOS 10.15 的 WKWebView 对应 **Safari 13**，相比现代版本缺少多项 JavaScript/Web API 支持。

## 问题分析与修复

### 1. Rust 编译目标不兼容

**问题**: 默认编译目标为 macOS 12+，生成的二进制无法在 10.15 上运行。

**修复**:
- `.cargo/config.toml` — 新建文件，设置 `MACOSX_DEPLOYMENT_TARGET = "10.15"`
- `.github/workflows/release.yml` — CI 环境添加 `MACOSX_DEPLOYMENT_TARGET: '10.15'`
- `src-tauri/tauri.conf.json` — `bundle.macOS.minimumSystemVersion` 从 `"12.0"` 改为 `"10.15"`

### 2. objc2 协议方法注册 panic (dev 模式)

**问题**: `objc2` 的 `define_class!` 宏在 debug 模式下会校验 ObjC 协议方法是否存在于当前系统。macOS 10.15 缺少 `WKDownload` 相关的 delegate 方法（macOS 11.3+ 引入），导致 panic：
```
failed overriding protocol method -[WKNavigationDelegate webView:navigationAction:didBecomeDownload:]: method not found
```

**修复**: `src-tauri/Cargo.toml` 添加：
```toml
[profile.dev.package.objc2]
debug-assertions = false
```
Release 模式本身就跳过这些检查，所以只需处理 dev 模式。

### 3. esbuild 版本不兼容

**问题**: esbuild 0.27+ 使用了 `_SecTrustCopyCertificateChain` 符号，该符号在 macOS 12+ 才可用，导致：
```
dyld: Symbol not found: _SecTrustCopyCertificateChain
```

**修复**: `package.json` 通过 pnpm overrides 锁定 esbuild 版本：
```json
"pnpm": {
  "overrides": {
    "esbuild": "0.21.5"
  }
}
```

### 4. ES2022 私有类字段语法 (`#field`)

**问题**: Vite 默认 build target 不够低，产物中保留了 `#privateField` 语法。Safari 13 的 WKWebView 不支持，报错：
```
SyntaxError: Invalid character: '#'
```

**修复**: `vite.config.ts` 添加 build target：
```ts
build: {
  target: ["es2020", "safari14"],
},
esbuild: {
  target: ["es2020", "safari14"],
},
```
> 注意：使用 `safari14` 而非 `safari13`，因为 `safari13` 会同时禁止 BigInt，但部分依赖库需要 BigInt 支持。`safari14` 足以降级私有字段语法。

### 5. smol-toml 中的 BigInt 字面量

**问题**: `smol-toml` 库源码中包含 BigInt 字面量（`0n`），无法被 esbuild 降级（这是语法层面的，不是 API）。Safari 13 不支持 BigInt，报错：
```
SyntaxError: No identifiers allowed directly after numeric literal
```

**修复**: 将 `smol-toml` 替换为 `@iarna/toml`（API 兼容，不使用 BigInt）。涉及 4 个文件：
- `src/utils/tomlUtils.ts`
- `src/utils/providerConfigUtils.ts`
- `src/components/providers/forms/hooks/useCodexCommonConfig.ts`
- `src/components/providers/forms/hooks/useCodexTomlValidation.ts`

### 6. MediaQueryList.addEventListener 不存在

**问题**: Safari 13 的 `MediaQueryList` 对象没有 `addEventListener` 方法（Safari 14+ 才支持），只有旧的 `addListener`。主题切换监听代码报错：
```
TypeError: Ki.addEventListener is not a function
```

**修复**: `src/components/theme-provider.tsx` 添加 fallback：
```ts
if (mediaQuery.addEventListener) {
  mediaQuery.addEventListener("change", handleChange);
  return () => mediaQuery.removeEventListener("change", handleChange);
}
// Safari 13 fallback
mediaQuery.addListener(handleChange);
return () => mediaQuery.removeListener(handleChange);
```

## 验证过的不需要改动

以下改动经过测试验证为**不必要**，已回退：

- **CSP 放宽** — Tauri 自定义协议在 10.15 上能正确匹配 `'self'`
- **framer-motion 降级** — v12 在 safari14 target 下编译后兼容 Safari 13
- **recharts 降级** — v3 在 safari14 target 下编译后兼容 Safari 13
- **App.tsx opacity 动画** — framer-motion 动画在 WKWebView 中正常工作

## 调试技巧

macOS 10.15 上调试 Tauri WKWebView 的方法：
1. `Cargo.toml` 中给 tauri 添加 `"devtools"` feature
2. Rust 代码中调用 `window.open_devtools()`
3. 通过 **Safari → 开发 → 应用进程** 连接 Web Inspector 查看 Console 错误
