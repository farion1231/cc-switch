// 「额度」：各应用在用的那家（中尺寸）。

import SwiftUI
import WidgetKit

struct AppRow: View {
    let app: WidgetApp

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

            if app.quotaLines.isEmpty {
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

    /// 中尺寸最多放三个应用：有额度的排前面，其余照托盘顺序。
    private func visibleApps(_ snapshot: WidgetSnapshot) -> [WidgetApp] {
        let withQuota = snapshot.apps.filter { !$0.quotaLines.isEmpty }
        let rest = snapshot.apps.filter { $0.quotaLines.isEmpty && $0.current != nil }
        return Array((withQuota + rest).prefix(3))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
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
                    ForEach(apps) { AppRow(app: $0) }
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
        .supportedFamilies([.systemMedium])
    }
}
