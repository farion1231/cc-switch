// 「额度」：各应用在用的那家和它的额度（托盘面板同一份数据）。四种尺寸，按应用翻页：
// 小尺寸一页一个应用（额度竖着排），中尺寸 3 个，大尺寸 7 个，特大两列共 14 个。

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

/// 小尺寸一页一个应用：图标、名字、在用的那家，下面额度竖着排。
struct SmallAppCard: View {
    let app: WidgetApp
    let displayUsed: Bool
    let now: Date

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                BrandLogo(icon: appIcon(app.app), fallback: app.name, size: 24)
                VStack(alignment: .leading, spacing: 1) {
                    Text(app.name)
                        .font(.system(size: 13, weight: .semibold))
                        .lineLimit(1)
                    if let current = app.current {
                        Text(current)
                            .font(.system(size: 10))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }
            if let tiers = app.tiers, !tiers.isEmpty {
                let rows = authQuotaRows(
                    tiers: tiers, resetCredits: nil, creditsBalance: nil,
                    displayUsed: displayUsed, now: now
                )
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(tightestRows(rows, limit: 3)) { CompactQuotaBars(rows: [$0]) }
                }
            } else if app.quotaLines.isEmpty {
                Text(app.quotaNote ?? app.attention ?? Strings.current.noQuota)
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                    .lineLimit(3)
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(app.quotaLines.prefix(3)) { QuotaBar(line: $0, compact: true) }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct QuotaWidgetView: View {
    let entry: SnapshotEntry
    /// 预览程序用：环境里的 widgetFamily 只读，没法从外面设。
    var familyOverride: WidgetFamily? = nil
    @Environment(\.widgetFamily) private var environmentFamily
    private var family: WidgetFamily { familyOverride ?? environmentFamily }

    /// 有额度的排前面，其余照托盘顺序（Antigravity 排最后）；没在用任何一家的不放。
    private func orderedApps(_ snapshot: WidgetSnapshot) -> [WidgetApp] {
        let hasQuota = { (app: WidgetApp) in !app.quotaLines.isEmpty || !(app.tiers ?? []).isEmpty }
        return snapshot.apps.filter(hasQuota) + snapshot.apps.filter { !hasQuota($0) && $0.current != nil }
    }

    /// 一列放几个、几列
    private var layout: (rows: Int, columns: Int) {
        switch family {
        case .systemSmall: return (1, 1)
        case .systemLarge: return (7, 1)
        case .systemExtraLarge: return (7, 2)
        default: return (3, 1)
        }
    }

    var body: some View {
        Group {
            if let snapshot = entry.snapshot {
                let apps = orderedApps(snapshot)
                if apps.isEmpty {
                    WidgetMessage(text: Strings.current.noQuota)
                } else {
                    content(apps, snapshot: snapshot)
                }
            } else {
                WidgetMessage(text: Strings.current.off)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private func content(_ apps: [WidgetApp], snapshot: WidgetSnapshot) -> some View {
        let layout = layout
        let pages = chunked(apps, size: layout.rows * layout.columns)
        let paging = Paging(kind: WidgetKind.quota, family: family, pages: pages.map { $0.map(\.id) })
        let page = pages[paging.current]
        let displayUsed = snapshot.quotaDisplay == "used"
        VStack(alignment: .leading, spacing: 0) {
            if family == .systemSmall, let app = page.first {
                SmallAppCard(app: app, displayUsed: displayUsed, now: entry.date)
            } else {
                HStack(alignment: .top, spacing: 24) {
                    // 两列时左右平分（7 个以内也分开放，别一列满一列空）
                    let perColumn = layout.columns == 2 ? (page.count + 1) / 2 : layout.rows
                    ForEach(Array(chunked(page, size: max(1, perColumn)).enumerated()), id: \.offset) { _, column in
                        VStack(alignment: .leading, spacing: family == .systemMedium ? 9 : 14) {
                            ForEach(column) { AppRow(app: $0, displayUsed: displayUsed, now: entry.date) }
                        }
                        .frame(maxWidth: .infinity, alignment: .topLeading)
                    }
                    // 特大尺寸只剩一个应用时，右边留空，别把一列拉满整个宽度
                    if layout.columns == 2, page.count == 1 {
                        Color.clear.frame(maxWidth: .infinity)
                    }
                }
            }
            Spacer(minLength: 4)
            WidgetFooter(paging: paging) {
                UpdatedTime(snapshot: snapshot, now: entry.date, compact: family == .systemSmall)
            }
        }
    }
}

struct QuotaWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: WidgetKind.quota, provider: SnapshotProvider()) { entry in
            QuotaWidgetView(entry: entry)
                .widgetGlassContent(.panel)
                .containerBackground(for: .widget) { WidgetGlassBackground(style: .panel) }
        }
        .configurationDisplayName(widgetDisplayName(Strings.current.quotaTitle))
        .description(Strings.current.quotaGallery)
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge, .systemExtraLarge])
    }
}
