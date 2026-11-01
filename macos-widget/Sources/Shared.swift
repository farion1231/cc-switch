// 三个小组件共用：时间线、玻璃底板、额度条、标题行、状态占位。

import SwiftUI
import WidgetKit

// MARK: - 时间线

struct SnapshotEntry: TimelineEntry {
    let date: Date
    let snapshot: WidgetSnapshot?
}

struct SnapshotProvider: TimelineProvider {
    /// 主程序每 10 分钟刷新一次，这里跟着重读。
    static let reloadInterval: TimeInterval = 10 * 60

    func placeholder(in context: Context) -> SnapshotEntry {
        SnapshotEntry(date: .now, snapshot: .sample)
    }

    func getSnapshot(in context: Context, completion: @escaping (SnapshotEntry) -> Void) {
        let snapshot = SnapshotStore.load()
        completion(SnapshotEntry(date: .now, snapshot: snapshot ?? (context.isPreview ? .sample : nil)))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<SnapshotEntry>) -> Void) {
        // 平时由主程序写完快照后通知重读（WidgetCenter）；这里的时间线只是兜底。
        let snapshot = SnapshotStore.load()
        var entries = [SnapshotEntry(date: .now, snapshot: snapshot)]
        // 主程序没开着、迟迟没有新快照：到点换成「数据过时」的样子，不用等重读
        if let snapshot {
            let staleAt = generatedTime(snapshot).addingTimeInterval(staleAfter)
            if staleAt > .now {
                entries.append(SnapshotEntry(date: staleAt, snapshot: snapshot))
            }
        }
        completion(Timeline(entries: entries, policy: .after(.now.addingTimeInterval(Self.reloadInterval))))
    }
}

// MARK: - 玻璃外观

/// 玻璃样式。
/// - `panel`：和菜单栏托盘面板同一种（`tray_panel.rs` 的 `NSGlassEffectView` Regular，
///   上面再盖 `tray-panel.css` 里 `.tp-card` 的底色），跟随浅色 / 深色。小组件默认用它。
/// - `clear`：macOS 26 桌面小组件的默认观感：透明、透出壁纸、白字。
/// - `regular`：不加底色的磨砂玻璃，跟随浅色 / 深色。
enum WidgetGlassStyle: String, CaseIterable {
    case panel
    case clear
    case regular
}

/// 托盘面板卡片的底色（`tray-panel.css`：浅色 rgba(246,246,248,0.72)，深色 rgba(30,30,33,0.62)）。
func panelTint(_ scheme: ColorScheme) -> Color {
    scheme == .dark
        ? Color(red: 30 / 255, green: 30 / 255, blue: 33 / 255).opacity(0.62)
        : Color(red: 246 / 255, green: 246 / 255, blue: 248 / 255).opacity(0.72)
}

/// 小组件底板。macOS 26 起用 Liquid Glass（`glassEffect`），更早的系统退回半透明填充。
/// 预览程序（Preview/Glass/main.swift）用同一个视图。
struct WidgetGlassBackground: View {
    var style: WidgetGlassStyle = .panel
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        if #available(macOS 26.0, *) {
            switch style {
            case .panel:
                ZStack {
                    Color.clear.glassEffect(.regular, in: ContainerRelativeShape())
                    ContainerRelativeShape().fill(panelTint(scheme))
                }
            case .clear:
                // 透明玻璃上压一层很淡的暗色，白字在亮壁纸上也读得清（系统透明小组件同样做法）
                Color.clear
                    .glassEffect(.clear.tint(.black.opacity(0.24)), in: ContainerRelativeShape())
            case .regular:
                Color.clear
                    .glassEffect(.regular, in: ContainerRelativeShape())
            }
        } else if style == .panel {
            // 托盘面板在旧系统上是 Popover 毛玻璃；小组件里没有窗口背后模糊，用同一层底色
            ZStack {
                Rectangle().fill(.regularMaterial)
                Rectangle().fill(panelTint(scheme))
            }
        } else {
            Rectangle().fill(.fill.tertiary)
        }
    }
}

extension View {
    /// 套上玻璃底板对应的文字配色：透明玻璃一律白字（按深色外观排），磨砂跟随系统。
    @ViewBuilder
    func widgetGlassContent(_ style: WidgetGlassStyle) -> some View {
        if style == .clear {
            environment(\.colorScheme, .dark)
        } else {
            self
        }
    }
}

// MARK: - 部件

func quotaTint(left: Double?, warn: Bool) -> Color {
    guard let left else { return .secondary }
    if left <= 0 { return .red }
    return warn ? .orange : .green
}

/// 一档额度：上面一行字，下面一根条。
struct QuotaBar: View {
    let line: QuotaLine
    var compact = false
    @Environment(\.widgetRenderingMode) private var renderingMode

    /// 着色 / 透明外观里系统会自己上色，自定义颜色只会被抹平。
    private var fullColor: Bool { renderingMode == .fullColor }
    private var tint: Color { quotaTint(left: line.left, warn: line.warn) }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 2 : 3) {
            Text(line.text)
                .font(.system(size: compact ? 9 : 10, weight: .medium).monospacedDigit())
                .foregroundStyle(line.warn && fullColor ? AnyShapeStyle(tint) : AnyShapeStyle(.secondary))
                .lineLimit(1)
            if let left = line.left {
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule().fill(.primary.opacity(0.12))
                        Capsule()
                            .fill(fullColor ? AnyShapeStyle(tint.gradient) : AnyShapeStyle(.primary))
                            .frame(width: max(compact ? 4 : 5, geo.size.width * max(0, min(left, 100)) / 100))
                            .widgetAccentable()
                    }
                }
                .frame(height: compact ? 4 : 5)
            }
        }
    }
}

/// 标题行：左边小组件名，右边附加信息（时间 / 账号数）。
struct WidgetHeader<Trailing: View>: View {
    let title: String
    @ViewBuilder var trailing: Trailing

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
            Spacer(minLength: 4)
            trailing
                .font(.system(size: 10).monospacedDigit())
                .foregroundStyle(.tertiary)
        }
    }
}

/// 没开小组件 / 主程序太旧 / 没数据时的居中提示。
struct WidgetMessage: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 小组件库里的名字。必须是普通 String：写成插值字面量会被当成 LocalizedStringKey（带格式的文字），
/// 系统读小组件列表时直接报「Formatted text for `configurationDisplayName` is not supported」崩溃。
func widgetDisplayName(_ title: String) -> String {
    "CC Switch · " + title
}

/// 快照多久没更新算过时：主程序每 10 分钟写一次，隔了两三轮还没新的，多半是 CC Switch 没开着。
let staleAfter: TimeInterval = 30 * 60

/// 快照写入时间；过时了标橙色并写「数据已过时」。
struct UpdatedTime: View {
    let snapshot: WidgetSnapshot
    let now: Date

    var body: some View {
        let time = generatedTime(snapshot)
        if now.timeIntervalSince(time) >= staleAfter {
            HStack(spacing: 3) {
                Image(systemName: "clock.badge.exclamationmark")
                Text(Strings.current.stale)
                Text(time, style: .time)
            }
            .foregroundStyle(.orange)
        } else {
            Text(time, style: .time)
        }
    }
}

/// 放不下时间的地方（用量小 / 中尺寸）：只在过时时出一个橙色时钟。
struct StaleBadge: View {
    let snapshot: WidgetSnapshot
    let now: Date

    var body: some View {
        if now.timeIntervalSince(generatedTime(snapshot)) >= staleAfter {
            Image(systemName: "clock.badge.exclamationmark")
                .foregroundStyle(.orange)
        }
    }
}

func generatedTime(_ snapshot: WidgetSnapshot) -> Date {
    Date(timeIntervalSince1970: snapshot.generatedAt)
}
