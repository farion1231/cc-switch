// 「额度」：各应用在用的那家（中尺寸）。

import SwiftUI
import WidgetKit

struct AppRow: View {
    let app: WidgetApp
    var displayUsed = false
    var now: Date = .now

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            BrandLogo(icon: appIcon(app.app), fallback: app.name, size: 18)
            VStack(alignment: .leading, spacing: 1) {
                Text(app.name)
                    .font(.system(size: 12, weight: .semibold))
                    .lineLimit(1)
                if let current = app.current {
                    Text(current)
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .frame(width: 112, alignment: .leading)

            if let tiers = app.tiers, !tiers.isEmpty {
                // Antigravity：按档位照授权中心写，挑最紧的两档
                let rows = authQuotaRows(
                    tiers: tiers, resetCredits: nil, creditsBalance: nil,
                    displayUsed: displayUsed, now: now
                )
                CompactQuotaBars(rows: tightestRows(rows, limit: 2))
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else if app.quotaLines.isEmpty {
                Text(app.quotaNote ?? app.attention ?? Strings.current.noQuota)
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                    .lineLimit(2)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                HStack(alignment: .top, spacing: 8) {
                    ForEach(app.quotaLines.prefix(2)) { line in
                        QuotaBar(line: line)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct QuotaWidgetView: View {
    let entry: SnapshotEntry
    /// 预览程序用：环境里的 widgetFamily 只读，没法从外面设。
    var familyOverride: WidgetFamily? = nil
    @Environment(\.widgetFamily) private var environmentFamily
    private var family: WidgetFamily { familyOverride ?? environmentFamily }

    /// 中尺寸放三个应用、大尺寸放全部：有额度的排前面，其余照托盘顺序（Antigravity 排最后）。
    private func visibleApps(_ snapshot: WidgetSnapshot) -> [WidgetApp] {
        let withQuota = snapshot.apps.filter { !$0.quotaLines.isEmpty || !($0.tiers ?? []).isEmpty }
        let rest = snapshot.apps.filter { $0.quotaLines.isEmpty && ($0.tiers ?? []).isEmpty && $0.current != nil }
        return Array((withQuota + rest).prefix(family == .systemLarge ? 8 : 3))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: family == .systemLarge ? 14 : 9) {
            WidgetHeader(title: "CC Switch · \(Strings.current.quotaTitle)") {
                if let snapshot = entry.snapshot {
                    UpdatedTime(snapshot: snapshot, now: entry.date)
                }
            }
            if let snapshot = entry.snapshot {
                let apps = visibleApps(snapshot)
                if apps.isEmpty {
                    WidgetMessage(text: Strings.current.noQuota)
                } else {
                    ForEach(apps) { AppRow(app: $0, displayUsed: snapshot.quotaDisplay == "used", now: entry.date) }
                    Spacer(minLength: 0)
                }
            } else {
                WidgetMessage(text: Strings.current.off)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

struct QuotaWidget: Widget {
    let kind = "com.ccswitch.desktop.widget.quota"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: SnapshotProvider()) { entry in
            QuotaWidgetView(entry: entry)
                .widgetGlassContent(.panel)
                .containerBackground(for: .widget) { WidgetGlassBackground(style: .panel) }
        }
        .configurationDisplayName(widgetDisplayName(Strings.current.quotaTitle))
        .description(Strings.current.quotaGallery)
        .supportedFamilies([.systemMedium, .systemLarge])
    }
}
