// 供应商 / 应用图标：和 app 里用的是同一套（sync_icons.py 从 src/icons/extracted 同步进
// Resources/Assets.xcassets）。单色图标是模板图，跟着文字颜色走；没有图标的退回首字母方块。

import AppKit
import SwiftUI
import WidgetKit

/// 授权中心渠道 → 图标名（同各 *Section.tsx 的 iconName）
func serviceIcon(_ service: String) -> String? {
    switch service {
    case "claude": return "claude"
    case "copilot": return "githubcopilot"
    case "chatgpt": return "openai"
    case "xai": return "xai"
    case "kimi": return "kimi"
    case "agy": return "antigravity"
    case "dsh": return "deepseek"
    default: return nil
    }
}

/// 应用（快照里的 app 键）→ 图标名
func appIcon(_ app: String) -> String? {
    switch app {
    case "claude", "claude-desktop": return "claude"
    case "codex": return "openai"
    case "gemini": return "gemini"
    case "grokbuild": return "grok"
    case "opencode": return "opencode-logo-light"
    case "openclaw": return "claw"
    case "hermes": return "hermes"
    case "mcode": return "minimax"
    case "kimi": return "kimi"
    case "dsh": return "deepseek"
    case "agy": return "antigravity"
    default: return nil
    }
}

/// 图标放在一块浅底圆角方块上（单色图标在玻璃上更稳），没有图标时写首字母。
struct BrandLogo: View {
    let icon: String?
    /// 没有图标时写它的首字母
    let fallback: String
    var size: CGFloat = 18

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
            .fill(.primary.opacity(0.08))
            .frame(width: size, height: size)
            .overlay {
                if let image = icon.flatMap(brandImage) {
                    image
                        .resizable()
                        .scaledToFit()
                        .padding(size * 0.16)
                        .foregroundStyle(.primary)
                } else {
                    Text(fallback.prefix(1).uppercased())
                        .font(.system(size: size * 0.52, weight: .bold, design: .rounded))
                        .foregroundStyle(.secondary)
                }
            }
            .widgetAccentable()
    }
}

/// 扩展里从 Assets.car 取；预览程序没有资源目录编译产物，直接读 SVG / PNG。
private func brandImage(_ name: String) -> Image? {
    #if CC_SWITCH_WIDGET_PREVIEW
    guard let dir = ProcessInfo.processInfo.environment["CCSW_RESOURCES"] else { return nil }
    let set = "\(dir)/Assets.xcassets/\(name).imageset"
    for ext in ["svg", "png"] {
        if let image = NSImage(contentsOfFile: "\(set)/\(name).\(ext)") {
            let contents = (try? String(contentsOfFile: "\(set)/Contents.json", encoding: .utf8)) ?? ""
            image.isTemplate = contents.contains("\"template\"")
            return Image(nsImage: image)
        }
    }
    return nil
    #else
    return NSImage(named: name) != nil ? Image(name) : nil
    #endif
}
