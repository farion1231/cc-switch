// 玻璃材质只有在真窗口里才渲染得出来（ImageRenderer 画不出背后的桌面）：
// 开一个铺着当前桌面壁纸的窗口，把三个小组件的各个尺寸都摆出来，可切浅色 / 深色、账号少 / 多。
// 数据按顺序取：命令行给的 JSON → 主程序写在 App Group 里的真实快照 → 示例数据。窗口顶部写着用的哪个。
// 用法：macos-widget/glass-preview.sh [widget-snapshot.json]

import AppKit
import SwiftUI
import WidgetKit

/// macOS 桌面小组件的大致尺寸（pt）和圆角。
enum PreviewSize {
    static let radius: CGFloat = 22
    static func of(_ family: WidgetFamily) -> CGSize {
        switch family {
        case .systemSmall: return CGSize(width: 170, height: 170)
        case .systemLarge: return CGSize(width: 364, height: 382)
        case .systemExtraLarge: return CGSize(width: 752, height: 382)
        default: return CGSize(width: 364, height: 170)
        }
    }
}

/// 预览外观：托盘面板同款（浅 / 深，小组件默认）、透明玻璃、磨砂浅色、磨砂深色。
enum PreviewLook: String, CaseIterable {
    case panelLight, panelDark, clear, light, dark

    var title: String {
        switch self {
        case .panelLight: return "托盘同款 · 浅"
        case .panelDark: return "托盘同款 · 深"
        case .clear: return "透明"
        case .light: return "磨砂 · 浅"
        case .dark: return "磨砂 · 深"
        }
    }

    var style: WidgetGlassStyle {
        switch self {
        case .panelLight, .panelDark: return .panel
        case .clear: return .clear
        case .light, .dark: return .regular
        }
    }
    var scheme: ColorScheme { self == .light || self == .panelLight ? .light : .dark }
}

struct PreviewTile<Content: View>: View {
    let family: WidgetFamily
    let label: String
    let look: PreviewLook
    @ViewBuilder var content: Content

    var body: some View {
        let size = PreviewSize.of(family)
        VStack(alignment: .leading, spacing: 6) {
            content
                .padding(16)
                .frame(width: size.width, height: size.height)
                .widgetGlassContent(look.style)
                .environment(\.colorScheme, look.scheme)
                .background(WidgetGlassBackground(style: look.style).environment(\.colorScheme, look.scheme))
                .containerShape(.rect(cornerRadius: PreviewSize.radius))
                .clipShape(.rect(cornerRadius: PreviewSize.radius))
            Text(label)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(.white)
                .shadow(radius: 2)
        }
    }
}

/// 预览用的数据和它的来源。
struct PreviewData {
    let snapshot: WidgetSnapshot
    /// nil = 示例数据
    let source: String?

    static func load() -> PreviewData {
        var candidates = CommandLine.arguments.dropFirst()
            .filter { !$0.hasPrefix("-") }
            .map { URL(fileURLWithPath: $0) }
        // glass-preview.sh 从编好的 appex 里读出 App Group 名字传进来
        if let group = ProcessInfo.processInfo.environment["CC_SWITCH_WIDGET_APP_GROUP"], !group.isEmpty {
            candidates.append(
                FileManager.default.homeDirectoryForCurrentUser
                    .appendingPathComponent("Library/Group Containers/\(group)/\(SnapshotStore.fileName)")
            )
        }
        for url in candidates {
            guard let data = try? Data(contentsOf: url) else { continue }
            do {
                return PreviewData(snapshot: try JSONDecoder().decode(WidgetSnapshot.self, from: data), source: url.path)
            } catch {
                print("读不懂 \(url.path): \(error)")
            }
        }
        return PreviewData(snapshot: .sample, source: nil)
    }
}

struct GalleryView: View {
    let data: PreviewData
    let wallpaper: NSImage?
    @State private var look = PreviewLook(rawValue: ProcessInfo.processInfo.environment["CCSW_LOOK"] ?? "") ?? .panelLight
    @State private var many = true

    var body: some View {
        // 只有示例数据才能切账号多少；真实快照原样显示
        let snapshot = data.source == nil
            ? data.snapshot.with(accounts: many ? WidgetSnapshot.sampleAccountsMany : WidgetSnapshot.sampleAccountsFew)
            : data.snapshot
        let entry = SnapshotEntry(date: .now, snapshot: snapshot)
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                // 截图模式（CCSW_CAPTURE）不画切换栏，只留小组件
                if ProcessInfo.processInfo.environment["CCSW_CAPTURE"] == nil {
                HStack(spacing: 16) {
                    Picker("", selection: $look) {
                        ForEach(PreviewLook.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 440)
                    if data.source == nil {
                        Toggle("16 accounts", isOn: $many)
                    }
                    Text(sourceLabel)
                        .font(.system(size: 11, weight: .medium))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .frame(maxWidth: 520, alignment: .leading)
                }
                .padding(10)
                .background(.regularMaterial, in: .capsule)
                }

                section("授权中心 · Auth Center") {
                    HStack(alignment: .top, spacing: 24) {
                        PreviewTile(family: .systemMedium, label: "Medium", look: look) {
                            AccountsWidgetView(entry: entry, familyOverride: .systemMedium)
                        }
                        PreviewTile(family: .systemLarge, label: "Large", look: look) {
                            AccountsWidgetView(entry: entry, familyOverride: .systemLarge)
                        }
                    }
                    PreviewTile(family: .systemExtraLarge, label: "Extra Large", look: look) {
                        AccountsWidgetView(entry: entry, familyOverride: .systemExtraLarge)
                    }
                }
                section("用量 · Usage") {
                    HStack(alignment: .top, spacing: 24) {
                        PreviewTile(family: .systemSmall, label: "Small", look: look) {
                            UsageWidgetView(entry: entry, familyOverride: .systemSmall)
                        }
                        PreviewTile(family: .systemMedium, label: "Medium", look: look) {
                            UsageWidgetView(entry: entry, familyOverride: .systemMedium)
                        }
                        PreviewTile(family: .systemLarge, label: "Large", look: look) {
                            UsageWidgetView(entry: entry, familyOverride: .systemLarge)
                        }
                    }
                }
                section("额度 · In-use quota") {
                    HStack(alignment: .top, spacing: 24) {
                        PreviewTile(family: .systemMedium, label: "Medium", look: look) {
                            QuotaWidgetView(entry: entry, familyOverride: .systemMedium)
                        }
                        PreviewTile(family: .systemLarge, label: "Large", look: look) {
                            QuotaWidgetView(entry: entry, familyOverride: .systemLarge)
                        }
                    }
                }
            }
            .padding(32)
        }
        .background {
            if let wallpaper {
                Image(nsImage: wallpaper).resizable().aspectRatio(contentMode: .fill)
            } else {
                LinearGradient(colors: [.indigo, .pink, .orange], startPoint: .topLeading, endPoint: .bottomTrailing)
            }
        }
        .frame(minWidth: 1220, minHeight: 860)
    }

    private var sourceLabel: String {
        guard let source = data.source else { return "示例数据（Sample.swift）" }
        let time = Date(timeIntervalSince1970: data.snapshot.generatedAt)
            .formatted(date: .abbreviated, time: .shortened)
        return "真实快照 · \(time) · \(source)"
    }

    private func section<C: View>(_ title: String, @ViewBuilder content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 15, weight: .bold))
                .foregroundStyle(.white)
                .shadow(radius: 3)
            content()
        }
    }
}

/// CGWindowListCreateImage 在新 SDK 里标成不可用，但系统里还在；只用来截自己的窗口。
private typealias WindowImageFn = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?

func captureOwnWindow(_ window: NSWindow, to url: URL) {
    guard let handle = dlopen(nil, RTLD_NOW),
          let sym = dlsym(handle, "CGWindowListCreateImage")
    else { print("CAPTURE_FAILED no symbol"); return }
    let fn = unsafeBitCast(sym, to: WindowImageFn.self)
    // kCGWindowListOptionIncludingWindow = 1 << 3，kCGWindowImageBoundsIgnoreFraming = 1 << 0
    guard let image = fn(.null, 1 << 3, UInt32(window.windowNumber), 1 << 0)?.takeRetainedValue() else {
        print("CAPTURE_FAILED nil image"); return
    }
    let rep = NSBitmapImageRep(cgImage: image)
    try? rep.representation(using: .png, properties: [:])?.write(to: url)
    print("CAPTURED \(url.path)")
}

final class PreviewDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    let data: PreviewData

    init(data: PreviewData) { self.data = data }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let wallpaper = NSScreen.main
            .flatMap { NSWorkspace.shared.desktopImageURL(for: $0) }
            .flatMap { NSImage(contentsOf: $0) }
        let hosting = NSHostingView(rootView: GalleryView(data: data, wallpaper: wallpaper))
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1220, height: 860),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered, defer: false
        )
        window.title = "CC Switch Widgets · Liquid Glass"
        window.contentView = hosting
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.window = window

        // CCSW_CAPTURE=<png>：等玻璃合成完，截自己的窗口存下来就退出（截自己的窗口不需要录屏权限）。
        if let out = ProcessInfo.processInfo.environment["CCSW_CAPTURE"] {
            // 截图时拉高窗口，一张图把三个小组件都装下
            window.setContentSize(NSSize(width: 1220, height: 1960))
            DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) {
                captureOwnWindow(window, to: URL(fileURLWithPath: out))
                NSApp.terminate(nil)
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = PreviewDelegate(data: PreviewData.load())
app.delegate = delegate
app.run()
