# CC Switch 国际化功能说明

## 已完成的工作

1. **安装依赖**：添加了 `react-i18next` 和 `i18next` 包
2. **配置国际化**：在 `src/i18n/` 目录下创建了配置文件
3. **翻译文件**：创建了英文和中文翻译文件
4. **组件更新**：替换了主要组件中的硬编码文案
5. **语言切换器**：添加了语言切换按钮

## 文件结构

```
src/
├── i18n/
│   ├── index.ts          # 国际化配置文件
│   └── locales/
│       ├── en.json       # 英文翻译
│       └── zh.json       # 中文翻译
├── components/
│   └── LanguageSwitcher.tsx  # 语言切换组件
└── main.tsx              # 导入国际化配置
```

## 默认语言设置

- **默认语言**：英文 (en)
- **回退语言**：英文 (en)

## 使用方式

1. 在组件中导入 `useTranslation`：
   ```tsx
   import { useTranslation } from 'react-i18next';

   function MyComponent() {
     const { t } = useTranslation();
     return <div>{t('common.save')}</div>;
   }
   ```

2. 切换语言：
   ```tsx
   const { i18n } = useTranslation();
   i18n.changeLanguage('zh'); // 切换到中文
   ```

## 翻译键结构

- `common.*` - 通用文案（保存、取消、设置等）
- `header.*` - 头部相关文案
- `provider.*` - 供应商相关文案
- `notifications.*` - 通知消息
- `settings.*` - 设置页面文案
- `apps.*` - 应用名称
- `console.*` - 控制台日志信息

## 测试功能

应用已添加了语言切换按钮（地球图标），点击可以在中英文之间切换，验证国际化功能是否正常工作。

## 已更新的组件

- ✅ App.tsx - 主应用组件
- ✅ ConfirmDialog.tsx - 确认对话框
- ✅ AddProviderModal.tsx - 添加供应商弹窗
- ✅ EditProviderModal.tsx - 编辑供应商弹窗
- ✅ ProviderList.tsx - 供应商列表
- ✅ LanguageSwitcher.tsx - 语言切换器
- 🔄 SettingsModal.tsx - 设置弹窗（部分完成）

## 注意事项

1. 所有新的文案都应该添加到翻译文件中，而不是硬编码
2. 翻译键名应该有意义且结构化
3. 可以通过修改 `src/i18n/index.ts` 中的 `lng` 配置来更改默认语言
