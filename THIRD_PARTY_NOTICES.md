# 来源与第三方审查状态

## 已确认的本地来源

- 基础项目：CC Switch，来源说明指向 https://github.com/farion1231/cc-switch 。
- 本地基线 commit：`adc223af322adaeca878e82bf78323865b0db768`。
- 本地 package / Cargo 版本：`4.0.15`。
- 根目录 [LICENSE](LICENSE) 原样保留，包含 `Copyright (c) 2025 Jason Young` 和 MIT 许可文本。
- 本轮修改：清理旧产品宣传、手册、发布与社区配置，新增 XSEC 项目文档。详细记录见 [仓库基线](docs/repository-baseline.md)。

## 未完成的审查

本文件是来源登记起点，**不是完整依赖许可证清单或商业分发结论**。

| 对象 | 证据入口 | 当前状态 | 发布前交付 |
| --- | --- | --- | --- |
| JavaScript 直接与传递依赖 | `package.json`、`pnpm-lock.yaml` | 未逐项审查 | 锁定版本、许可证文本、来源及分发义务清单 |
| Rust 直接与传递依赖 | `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` | 未逐项审查 | 各目标平台依赖及许可证清单 |
| 运行时图标、字体及图片 | `src/assets/`、`src/icons/`、`src-tauri/icons/` | 保留，未逐项审查 | 来源、授权证据、需替换项 |
| 打包和平台资源 | `src-tauri/wix/`、Tauri 配置及平台文件 | 保留工程基础 | 新品牌、签名主体及资源来源核对 |
| 清理的旧产品资料 | [清理清单](docs/cleanup-manifest.tsv) | 已从工作树移除，Git 基线可恢复 | 不打包旧产品宣传与发布资料 |

清理不删除 Git 历史、不修改作者信息，也不代表取得上游商标或第三方资源授权。新增依赖时补充可核查来源，完整审查作为正式发布门禁。
