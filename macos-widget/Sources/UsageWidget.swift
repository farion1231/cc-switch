// 「用量」：用量统计页同一份数据。时间范围和样式由用户在「编辑小组件」里选（UsageIntent.swift）：
// - 概览：大数字花费 + 明细 + 趋势；大尺寸再加按应用拆分（默认）
// - 趋势：以趋势图为主
// - 按应用：各应用花费排行
// - 指标：花费 / Token / 请求 / 缓存命中四个数

import Charts
import SwiftUI
import WidgetKit

func compactNumber(_ value: Int) -> String {
    let v = Double(value)
    switch v {
    case 1_000_000_000...: return String(format: "%.1fB", v / 1_000_000_000)
    case 1_000_000...: return String(format: "%.1fM", v / 1_000_000)
    case 1_000...: return String(format: "%.1fK", v / 1_000)
    default: return "\(value)"
    }
}

// MARK: - 部件

/// 趋势图。柱状（概览）或面积（趋势样式）；横轴按粒度写小时 / 星期 / 日期 / 月份。
struct UsageChart: View {
    enum Kind { case bars, area }

    let points: [UsagePoint]
    let granularity: String
    var kind: Kind = .bars
    var showAxis = false
    @Environment(\.widgetRenderingMode) private var renderingMode

    private var unit: Calendar.Component {
        switch granularity {
        case "hour": return .hour
        case "week": return .weekOfYear
        case "month": return .month
        default: return .day
        }
    }

    private var fullColor: Bool { renderingMode == .fullColor }

    var body: some View {
        let last = points.last?.start
        Chart(points) { point in
            switch kind {
            case .bars:
                BarMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .cornerRadius(2)
                .foregroundStyle(
                    fullColor
                        ? AnyShapeStyle(point.start == last ? Color.accentColor.gradient : Color.accentColor.opacity(0.45).gradient)
                        : AnyShapeStyle(.primary)
                )
            case .area:
                AreaMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .interpolationMethod(.monotone)
                .foregroundStyle(
                    fullColor
                        ? AnyShapeStyle(LinearGradient(colors: [Color.accentColor.opacity(0.45), Color.accentColor.opacity(0.04)], startPoint: .top, endPoint: .bottom))
                        : AnyShapeStyle(.primary.opacity(0.3))
                )
                LineMark(
                    x: .value("time", point.date, unit: unit),
                    y: .value("cost", point.costValue)
                )
                .interpolationMethod(.monotone)
                .lineStyle(StrokeStyle(lineWidth: 1.6))
                .foregroundStyle(fullColor ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(.primary))
            }
        }
        .chartYAxis(.hidden)
        .chartXAxis {
            if showAxis { axis }
        }
        .widgetAccentable()
    }

    /// 不画网格线，玻璃上太乱；标签数量控制在 4–7 个。
    @AxisContentBuilder
    private var axis: some AxisContent {
        switch granularity {
        case "hour":
            AxisMarks(preset: .aligned, values: .stride(by: .hour, count: 6)) { _ in
                AxisValueLabel(format: .dateTime.hour())
            }
        case "month":
            AxisMarks(preset: .aligned, values: .stride(by: .month, count: max(1, points.count / 5))) { _ in
                AxisValueLabel(format: .dateTime.year(.twoDigits).month(.defaultDigits))
            }
        case "week":
            AxisMarks(preset: .aligned, values: .stride(by: .weekOfYear, count: max(1, points.count / 5))) { _ in
                AxisValueLabel(format: .dateTime.month(.defaultDigits).day())
            }
        default:
            if points.count <= 7 {
                AxisMarks(preset: .aligned, values: .stride(by: .day)) { _ in
                    AxisValueLabel(format: .dateTime.weekday(.narrow), centered: true)
                }
            } else {
                AxisMarks(preset: .aligned, values: .stride(by: .day, count: points.count <= 14 ? 3 : 7)) { _ in
                    AxisValueLabel(format: .dateTime.month(.defaultDigits).day())
                }
            }
        }
    }
}

struct Metric: View {
    let value: String
    let label: String
    var size: CGFloat = 13

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(value)
                .font(.system(size: size, weight: .semibold, design: .rounded).monospacedDigit())
                .lineLimit(1)
                .minimumScaleFactor(0.6)
            Text(label)
                .font(.system(size: 9))
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

/// 按应用拆分的一行：名字、比例条、金额。
struct AppShareRow: View {
    let app: UsageByApp
    let maxCost: Double
    var nameWidth: CGFloat = 92

    var body: some View {
        HStack(spacing: 8) {
            BrandLogo(icon: appIcon(app.app), fallback: app.name, size: 15)
            Text(app.name)
                .font(.system(size: 11, weight: .medium))
                .lineLimit(1)
                .frame(width: nameWidth, alignment: .leading)
            GeometryReader { geo in
                Capsule()
                    .fill(Color.accentColor.gradient)
                    .frame(width: max(4, geo.size.width * app.costValue / max(maxCost, 0.0001)))
                    .widgetAccentable()
            }
            .frame(height: 6)
            // 金额不换行、不截断：按自身宽度排，比例条让位（「全部」里常见 $24,535.49 这种长金额）
            Text(app.cost)
                .font(.system(size: 10, weight: .medium).monospacedDigit())
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .fixedSize()
                .layoutPriority(1)
        }
    }
}

// MARK: - 小组件视图

struct UsageWidgetView: View {
    let entry: SnapshotEntry
    var range: UsageRangeOption = .today
    var style: UsageStyleOption = .overview
    /// 预览程序用：环境里的 widgetFamily 只读，没法从外面设。
    var familyOverride: WidgetFamily? = nil
    @Environment(\.widgetFamily) private var environmentFamily
    private var family: WidgetFamily { familyOverride ?? environmentFamily }

    var body: some View {
        Group {
            if let snapshot = entry.snapshot {
                if let data = snapshot.usage?.ranges?[range.rawValue] {
                    content(data, snapshot: snapshot)
                } else {
                    // 旧版主程序（快照版本 1）没有时间范围；新版是用量统计读失败了
                    message(snapshot.version >= 2 ? Strings.current.noUsage : Strings.current.outdated)
                }
            } else {
                message(Strings.current.off)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func message(_ text: String) -> some View {
        VStack(alignment: .leading) {
            WidgetHeader(title: Strings.current.usageTitle) { EmptyView() }
            WidgetMessage(text: text)
        }
    }

    /// 标题：「用量 · 近 7 天」，右边过时标记（大尺寸写时间）。
    private func header(_ snapshot: WidgetSnapshot) -> some View {
        WidgetHeader(title: "\(Strings.current.usageTitle) · \(range.title)") {
            if family == .systemLarge {
                UpdatedTime(snapshot: snapshot, now: entry.date)
            } else {
                StaleBadge(snapshot: snapshot, now: entry.date)
            }
        }
    }

    private func bigCost(_ text: String, size: CGFloat) -> some View {
        Text(text)
            .font(.system(size: size, weight: .bold, design: .rounded).monospacedDigit())
            .lineLimit(1)
            .minimumScaleFactor(0.5)
    }

    private func tokensLine(_ totals: UsageTotals) -> some View {
        Text("\(compactNumber(totals.tokens)) \(Strings.current.tokens) · \(totals.requests.formatted()) \(Strings.current.requests)")
            .font(.system(size: 10).monospacedDigit())
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .minimumScaleFactor(0.8)
    }

    private func metrics(_ totals: UsageTotals, size: CGFloat = 13) -> [Metric] {
        [
            Metric(value: totals.cost, label: Strings.current.cost, size: size),
            Metric(value: compactNumber(totals.tokens), label: Strings.current.tokens, size: size),
            Metric(value: totals.requests.formatted(), label: Strings.current.requests, size: size),
            Metric(value: "\(Int((totals.cacheHitRate * 100).rounded()))%", label: Strings.current.cacheHit, size: size),
        ]
    }

    @ViewBuilder
    private func content(_ data: UsageRange, snapshot: WidgetSnapshot) -> some View {
        switch style {
        case .overview: overview(data, snapshot: snapshot)
        case .trend: trend(data, snapshot: snapshot)
        case .apps: apps(data, snapshot: snapshot)
        case .metrics: metricsStyle(data, snapshot: snapshot)
        }
    }

    // 概览
    @ViewBuilder
    private func overview(_ data: UsageRange, snapshot: WidgetSnapshot) -> some View {
        switch family {
        case .systemSmall:
            VStack(alignment: .leading, spacing: 2) {
                header(snapshot)
                Spacer(minLength: 2)
                bigCost(data.totals.cost, size: 30)
                tokensLine(data.totals)
                UsageChart(points: data.series, granularity: data.granularity)
                    .frame(height: 34)
                    .padding(.top, 6)
            }
        case .systemLarge:
            VStack(alignment: .leading, spacing: 10) {
                header(snapshot)
                bigCost(data.totals.cost, size: 34)
                HStack(spacing: 16) {
                    ForEach(Array(metrics(data.totals).dropFirst().enumerated()), id: \.offset) { $0.element }
                }
                UsageChart(points: data.series, granularity: data.granularity, showAxis: true)
                    .frame(height: 110)
                Divider().opacity(0.5)
                appList(data, limit: 4)
                Spacer(minLength: 0)
            }
        default:
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    header(snapshot)
                    bigCost(data.totals.cost, size: 30)
                    Spacer(minLength: 0)
                    // 三个数字（英文「Cache hits」等较长）：给足宽度、可缩小，别和右边图表的坐标轴挤在一起
                    HStack(spacing: 10) {
                        ForEach(Array(metrics(data.totals, size: 12).dropFirst().enumerated()), id: \.offset) {
                            $0.element.fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .minimumScaleFactor(0.8)
                }
                .frame(width: 168, alignment: .leading)
                UsageChart(points: data.series, granularity: data.granularity, showAxis: true)
            }
        }
    }

    // 趋势
    @ViewBuilder
    private func trend(_ data: UsageRange, snapshot: WidgetSnapshot) -> some View {
        VStack(alignment: .leading, spacing: family == .systemSmall ? 4 : 8) {
            header(snapshot)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                bigCost(data.totals.cost, size: family == .systemSmall ? 20 : 24)
                if family != .systemSmall {
                    tokensLine(data.totals)
                }
            }
            UsageChart(points: data.series, granularity: data.granularity, kind: .area, showAxis: family != .systemSmall)
        }
    }

    // 按应用
    @ViewBuilder
    private func apps(_ data: UsageRange, snapshot: WidgetSnapshot) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            header(snapshot)
            HStack(alignment: .firstTextBaseline) {
                bigCost(data.totals.cost, size: family == .systemSmall ? 18 : 22)
                Spacer()
            }
            if data.byApp.isEmpty {
                WidgetMessage(text: Strings.current.noUsage)
            } else if family == .systemSmall {
                let top = Array(data.byApp.prefix(3))
                VStack(alignment: .leading, spacing: 5) {
                    ForEach(top) { app in
                        HStack {
                            Text(app.name).font(.system(size: 10, weight: .medium)).lineLimit(1)
                            Spacer(minLength: 4)
                            Text(app.cost).font(.system(size: 10).monospacedDigit()).foregroundStyle(.secondary)
                                .lineLimit(1).fixedSize().layoutPriority(1)
                        }
                    }
                }
                Spacer(minLength: 0)
            } else {
                appList(data, limit: family == .systemLarge ? 9 : 3)
                Spacer(minLength: 0)
            }
        }
    }

    private func appList(_ data: UsageRange, limit: Int) -> some View {
        let maxCost = data.byApp.map(\.costValue).max() ?? 1
        return VStack(spacing: 7) {
            ForEach(data.byApp.prefix(limit)) { app in
                AppShareRow(app: app, maxCost: maxCost)
            }
        }
    }

    // 指标
    @ViewBuilder
    private func metricsStyle(_ data: UsageRange, snapshot: WidgetSnapshot) -> some View {
        let items = metrics(data.totals, size: family == .systemSmall ? 15 : 18)
        VStack(alignment: .leading, spacing: 10) {
            header(snapshot)
            if family == .systemMedium {
                HStack(alignment: .top, spacing: 18) {
                    ForEach(Array(items.enumerated()), id: \.offset) { $0.element }
                }
                UsageChart(points: data.series, granularity: data.granularity, kind: .area)
            } else {
                Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 12) {
                    GridRow { items[0]; items[1] }
                    GridRow { items[2]; items[3] }
                }
                if family == .systemLarge {
                    UsageChart(points: data.series, granularity: data.granularity, kind: .area, showAxis: true)
                } else {
                    Spacer(minLength: 0)
                }
            }
        }
    }
}

struct UsageWidget: Widget {
    let kind = "com.ccswitch.desktop.widget.usage"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: kind, intent: UsageWidgetIntent.self, provider: UsageIntentProvider()) { entry in
            UsageWidgetView(
                entry: SnapshotEntry(date: entry.date, snapshot: entry.snapshot),
                range: entry.range,
                style: entry.style
            )
            .widgetGlassContent(.panel)
            .containerBackground(for: .widget) { WidgetGlassBackground(style: .panel) }
        }
        .configurationDisplayName(widgetDisplayName(Strings.current.usageTitle))
        .description(Strings.current.usageGallery)
        .supportedFamilies([.systemSmall, .systemMedium, .systemLarge])
    }
}
