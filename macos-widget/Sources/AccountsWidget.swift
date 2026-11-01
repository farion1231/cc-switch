// 「授权中心」：全部托管账号，每个账号按授权中心那一行显示（规则在 AuthCenterText.swift）：
// 账号名 +「默认」/「需要重新登录」标签，第二行是各渠道自己的信息（套餐、域名、区域、日期、
// 「N 个供应商在用」），右边 / 下面是额度：每档一行（档名、条、剩余、倒计时），ChatGPT 再加
// 重置次数和 Credits，查询失败写原因，Kimi 没订阅写「未订阅」。
//
// 账号多时按 `prioritized` 排序——要处理的、额度最紧的先上；放不下的折成「还有 N 个」。
// 中尺寸放不下整块额度：每个号一行，只画最紧的两档；要处理的账号合成一行提醒。

import SwiftUI
import WidgetKit

/// 渠道图标：和授权中心各分组用的是同一个供应商图标（BrandLogo.swift）。
struct ServiceMark: View {
    let service: String
    var size: CGFloat = 18

    var body: some View {
        BrandLogo(icon: serviceIcon(service), fallback: service, size: size)
    }
}

/// 小标签（「默认」「需要重新登录」「在用」）
struct Pill: View {
    let text: String
    let color: Color
    var filled = true

    var body: some View {
        Text(text)
            .font(.system(size: 8.5, weight: .medium))
            .lineLimit(1)
            .padding(.horizontal, 4)
            .padding(.vertical, 1)
            .background {
                if filled {
                    Capsule().fill(color.opacity(0.18))
                } else {
                    Capsule().strokeBorder(color.opacity(0.5), lineWidth: 0.5)
                }
            }
            .foregroundStyle(color)
    }
}

/// 授权中心小组件的固定尺寸。每个元素都按这些行高画，排版时就能按真实高度算放几个号，
/// 不会把标题和底栏挤出去。
enum AuthLayout {
    static let titleHeight: CGFloat = 20
    static let detailHeight: CGFloat = 15
    static let rowHeight: CGFloat = 16
    static let innerGap: CGFloat = 3
    static let blockSpacing: CGFloat = 12
    static let indent: CGFloat = 24
    static let headerHeight: CGFloat = 16
    static let footerHeight: CGFloat = 14
    static let sectionSpacing: CGFloat = 10
    /// 中尺寸一行
    static let compactRowHeight: CGFloat = 30
    static let compactSpacing: CGFloat = 9
}

/// 额度列的一行：档名 | 条（或说明）| 数值 | 倒计时
struct AuthQuotaRowView: View {
    let row: AuthQuotaRow
    let now: Date
    var labelWidth: CGFloat = 96
    @Environment(\.widgetRenderingMode) private var renderingMode

    var body: some View {
        HStack(spacing: 8) {
            Text(row.label)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .frame(width: labelWidth, alignment: .leading)
            if let bar = row.bar {
                GeometryReader { geo in
                    ZStack(alignment: .leading) {
                        Capsule().fill(.primary.opacity(0.12))
                        Capsule()
                            .fill(renderingMode == .fullColor ? AnyShapeStyle(row.tone.color.gradient) : AnyShapeStyle(.primary))
                            .frame(width: max(3, geo.size.width * max(0, min(bar, 100)) / 100))
                            .widgetAccentable()
                    }
                }
                .frame(height: 5)
            } else {
                Text(row.caption ?? "")
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            Text(row.value)
                .monospacedDigit()
                .foregroundStyle(row.tone == .normal || renderingMode != .fullColor ? AnyShapeStyle(.primary) : AnyShapeStyle(row.tone.color))
                .lineLimit(1)
                .fixedSize()
            // 倒计时格始终留着，各行的条长度一致
            Text(countdown(row.resetsAt, now: now) ?? "")
                .monospacedDigit()
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .frame(width: 44, alignment: .trailing)
        }
        .font(.system(size: 10))
        .frame(height: AuthLayout.rowHeight)
    }
}

/// 一个账号在大 / 特大尺寸里的整块：标题行、第二行、额度列（最多 `maxRows` 行）。
struct AccountBlock: View {
    let account: WidgetAccount
    let displayUsed: Bool
    let now: Date
    /// 一个号最多画几行额度：大尺寸 2，特大 4；多出来的写「还有 N 档」
    var maxRows = 4

    /// 要画的额度行：放不下时挑剩余最少的几档（顺序照原来的），重置次数和 Credits 排在后面
    private var quotaRows: (shown: [AuthQuotaRow], hidden: Int) {
        guard case let .rows(tiers, resetCredits, credits, _) = account.quota else { return ([], 0) }
        let rows = authQuotaRows(
            tiers: tiers, resetCredits: resetCredits, creditsBalance: credits,
            displayUsed: displayUsed, now: now
        )
        guard rows.count > maxRows else { return (rows, 0) }
        // 留一行写「还有 N 档」
        let shown = tightestRows(rows, limit: maxRows - 1)
        return (shown, rows.count - shown.count)
    }

    private var details: String { accountDetailsLine(account, now: now) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                ServiceMark(service: account.service, size: 18)
                Text(account.displayLogin)
                    .font(.system(size: 12.5, weight: .semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .privacySensitive()
                if account.isDefault {
                    Pill(text: defaultLabel(account), color: .secondary, filled: false)
                }
                if account.needsReauth {
                    Pill(text: T("authCenter.needsReauth"), color: .red)
                }
                if account.inUse, !account.isDefault {
                    Pill(text: Strings.current.inUse, color: .green)
                }
                Spacer(minLength: 0)
            }
            .frame(height: AuthLayout.titleHeight)
            if !details.isEmpty {
                Text(details)
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .frame(height: AuthLayout.detailHeight)
                    .padding(.leading, AuthLayout.indent)
            }
            quota
                .padding(.leading, AuthLayout.indent)
                .padding(.top, quotaLineCount > 0 ? AuthLayout.innerGap : 0)
        }
    }

    private var quotaLineCount: Int {
        switch account.quota {
        case .rows:
            let rows = quotaRows
            return rows.shown.count + (rows.hidden > 0 ? 1 : 0)
        case .failed, .note: return 1
        case let .balance(_, bonus): return bonus.isEmpty ? 1 : 2
        case .none: return 0
        }
    }

    @ViewBuilder
    private var quota: some View {
        switch account.quota {
        case .rows:
            let rows = quotaRows
            VStack(alignment: .leading, spacing: 0) {
                ForEach(rows.shown) { AuthQuotaRowView(row: $0, now: now) }
                if rows.hidden > 0 {
                    Text(String(format: Strings.current.moreTiers, rows.hidden))
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                        .frame(height: AuthLayout.rowHeight)
                }
            }
        case let .failed(kind, message) where account.service == "dsh":
            // DshAccount.tsx：余额查不到时只写一句
            Text(T("dsh.account.balanceFailed"))
                .font(.system(size: 10))
                .foregroundStyle(.tertiary)
                .frame(height: AuthLayout.rowHeight)
                .help(quotaFailureText(kind: kind, message: message))
        case let .failed(kind, message):
            HStack(spacing: 4) {
                Text(T("quota.failed")).foregroundStyle(.red).fontWeight(.medium)
                Text(quotaFailureText(kind: kind, message: message))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }
            .font(.system(size: 10))
            .frame(height: AuthLayout.rowHeight)
        case let .note(note):
            Text(note == "noSubscription" ? T("kimi.oauth.noSubscription") : note)
                .font(.system(size: 10))
                .foregroundStyle(.tertiary)
                .frame(height: AuthLayout.rowHeight)
        case let .balance(wallets, bonus):
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(dshBalanceLines(wallets: wallets, bonus: bonus).enumerated()), id: \.offset) { index, line in
                    Text(line)
                        .font(.system(size: 10).monospacedDigit())
                        .foregroundStyle(index == 0 ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                        .frame(height: AuthLayout.rowHeight)
                }
            }
        case .none:
            EmptyView()
        }
    }

    /// 和 body 用同一组行高算出来的高度（排版时决定放几个号）
    var height: CGFloat {
        var height = AuthLayout.titleHeight
        if !details.isEmpty { height += AuthLayout.detailHeight }
        let lines = quotaLineCount
        if lines > 0 { height += AuthLayout.innerGap + CGFloat(lines) * AuthLayout.rowHeight }
        return height
    }
}

/// 中尺寸的一行：图标、账号名、最紧的两档（或状态）。
struct CompactAccountRow: View {
    let account: WidgetAccount
    let displayUsed: Bool
    let now: Date
    @Environment(\.widgetRenderingMode) private var renderingMode

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            ServiceMark(service: account.service, size: 18)
            VStack(alignment: .leading, spacing: 2) {
                Text(account.displayLogin)
                    .font(.system(size: 11.5, weight: .semibold))
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .privacySensitive()
                Text(account.serviceName)
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(.secondary)
            }
            .frame(width: 104, alignment: .leading)
            trailing.frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(height: AuthLayout.compactRowHeight)
    }

    @ViewBuilder
    private var trailing: some View {
        switch account.quota {
        case let .rows(tiers, resetCredits, credits, _):
            let rows = authQuotaRows(
                tiers: tiers, resetCredits: resetCredits, creditsBalance: credits,
                displayUsed: displayUsed, now: now
            )
            // 放得下两档：挑剩余最少的两档，顺序照原来的；和 app 卡片一样不放「每周 Sonnet」
            let bars = tightestRows(
                rows.filter { $0.bar != nil && $0.id != "seven_day_sonnet" }, limit: 2
            )
            CompactQuotaBars(rows: bars)
        case let .balance(wallets, bonus):
            Text(dshBalanceLines(wallets: wallets, bonus: bonus).joined(separator: " · "))
                .font(.system(size: 10).monospacedDigit())
                .lineLimit(1)
        case let .failed(kind, message):
            Text("\(T("quota.failed")) · \(quotaFailureText(kind: kind, message: message))")
                .font(.system(size: 10))
                .foregroundStyle(.red)
                .lineLimit(1)
        case let .note(note):
            Text(note == "noSubscription" ? T("kimi.oauth.noSubscription") : note)
                .font(.system(size: 9.5))
                .foregroundStyle(.tertiary)
        case .none:
            if account.needsReauth {
                Text(T("authCenter.needsReauth"))
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(.red)
            } else {
                Text(accountDetailsLine(account, now: now))
                    .font(.system(size: 9.5))
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }
        }
    }
}

/// 中尺寸放的两根短额度条：「5 小时 62%」+ 条。额度小组件的应用行也用它。
struct CompactQuotaBars: View {
    let rows: [AuthQuotaRow]
    @Environment(\.widgetRenderingMode) private var renderingMode

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            ForEach(rows) { row in
                VStack(alignment: .leading, spacing: 3) {
                    Text(row.short)
                        .font(.system(size: 9.5, weight: .medium).monospacedDigit())
                        .minimumScaleFactor(0.8)
                        .foregroundStyle(row.tone == .normal ? AnyShapeStyle(.secondary) : AnyShapeStyle(row.tone.color))
                        .lineLimit(1)
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Capsule().fill(.primary.opacity(0.12))
                            Capsule()
                                .fill(renderingMode == .fullColor ? AnyShapeStyle(row.tone.color.gradient) : AnyShapeStyle(.primary))
                                .frame(width: max(3, geo.size.width * (row.bar ?? 0) / 100))
                                .widgetAccentable()
                        }
                    }
                    .frame(height: 5)
                }
            }
        }
    }
}

/// DeepSeek 账号的余额、赠金两行（DshAccount.tsx 的 DshBalance）
func dshBalanceLines(wallets: [String], bonus: [String]) -> [String] {
    var lines = [T("dsh.account.balance", ["amount": wallets.isEmpty ? "—" : wallets.joined(separator: " · ")])]
    if !bonus.isEmpty {
        lines.append(T("dsh.account.bonus", ["amount": bonus.joined(separator: " · ")]))
    }
    return lines
}

extension QuotaTone {
    /// 排序用：越紧越靠前（条长按剩余算）
    static func rank(_ row: AuthQuotaRow) -> Double {
        switch row.tone {
        case .danger: return -1
        case .warning: return 0
        case .normal: return 1
        }
    }
}

/// 中尺寸里把要处理的账号合成一行：「⚠ 3 个要处理 · side@gmail.com、old-pro@…」。
struct PendingAlert: View {
    let accounts: [WidgetAccount]

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "exclamationmark.triangle.fill")
            Text(String(format: Strings.current.needAction, accounts.count))
                .fontWeight(.semibold)
            Text(accounts.map(\.login).joined(separator: ", "))
                .foregroundStyle(.orange.opacity(0.8))
                .lineLimit(1)
                .truncationMode(.tail)
                .privacySensitive()
        }
        .font(.system(size: 10))
        .foregroundStyle(.orange)
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.orange.opacity(0.14), in: .rect(cornerRadius: 8))
    }
}

struct AccountsWidgetView: View {
    let entry: SnapshotEntry
    /// 预览程序用：环境里的 widgetFamily 只读，没法从外面设。
    var familyOverride: WidgetFamily? = nil
    @Environment(\.widgetFamily) private var environmentFamily
    private var family: WidgetFamily { familyOverride ?? environmentFamily }

    var body: some View {
        VStack(alignment: .leading, spacing: AuthLayout.sectionSpacing) {
            if let snapshot = entry.snapshot {
                if let accounts = snapshot.accounts {
                    content(prioritized(accounts), snapshot: snapshot)
                } else {
                    header(nil)
                    // 旧版主程序（快照版本 1）不写账号段；新版只是刚启动还没读到
                    WidgetMessage(text: snapshot.version >= 2 ? Strings.current.loadingAccounts : Strings.current.outdated)
                }
            } else {
                header(nil)
                WidgetMessage(text: Strings.current.off)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    /// 标题行：账号数、要处理的数；中尺寸没有底栏，「还有 N 个」写在这里
    /// 标题行：账号数、要处理的数；右边再挂过时标记（中尺寸没有底栏，只能看这里）
    private func header(_ accounts: [WidgetAccount]?, snapshot: WidgetSnapshot? = nil) -> some View {
        WidgetHeader(title: "CC Switch · \(Strings.current.accountsTitle)") {
            HStack(spacing: 6) {
                if let accounts, !accounts.isEmpty {
                    let pending = accounts.filter(\.needsAction).count
                    Text(Strings.current.accounts(accounts.count))
                    // 中尺寸下面有一行提醒，标题里不再重复
                    if pending > 0, family != .systemMedium {
                        Text(String(format: Strings.current.needAction, pending))
                            .foregroundStyle(.orange)
                    }
                }
                if family == .systemMedium, let snapshot {
                    StaleBadge(snapshot: snapshot, now: entry.date)
                }
            }
        }
        .frame(height: AuthLayout.headerHeight)
    }

    @ViewBuilder
    private func content(_ accounts: [WidgetAccount], snapshot: WidgetSnapshot) -> some View {
        let displayUsed = snapshot.quotaDisplay == "used"
        if accounts.isEmpty {
            header(accounts)
            WidgetMessage(text: Strings.current.noAccounts)
        } else if family == .systemMedium {
            medium(accounts, snapshot: snapshot, displayUsed: displayUsed)
        } else {
            let columns = family == .systemExtraLarge ? 2 : 1
            let maxRows = family == .systemExtraLarge ? 4 : 2
            header(accounts)
            // 按真实可用高度装：放得下几个放几个，剩下的写进底栏
            GeometryReader { geo in
                let packed = pack(accounts, columns: columns, maxRows: maxRows, displayUsed: displayUsed, height: geo.size.height)
                VStack(alignment: .leading, spacing: 0) {
                    HStack(alignment: .top, spacing: 20) {
                        ForEach(Array(packed.columns.enumerated()), id: \.offset) { _, column in
                            VStack(alignment: .leading, spacing: AuthLayout.blockSpacing) {
                                ForEach(column, id: \.uid) { account in
                                    AccountBlock(account: account, displayUsed: displayUsed, now: entry.date, maxRows: maxRows)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .topLeading)
                        }
                    }
                    Spacer(minLength: 0)
                    footer(hidden: accounts.count - packed.shown, snapshot: snapshot)
                }
            }
        }
    }

    /// 按各块的真实高度把账号依次装进各列，装不下的不放。
    private func pack(
        _ accounts: [WidgetAccount], columns: Int, maxRows: Int, displayUsed: Bool, height: CGFloat
    ) -> (columns: [[WidgetAccount]], shown: Int) {
        let budget = height - AuthLayout.footerHeight - AuthLayout.sectionSpacing
        var result = Array(repeating: [WidgetAccount](), count: columns)
        var column = 0
        var used: CGFloat = 0
        var shown = 0
        for account in accounts {
            let block = AccountBlock(account: account, displayUsed: displayUsed, now: entry.date, maxRows: maxRows).height
            let needed = (result[column].isEmpty ? 0 : AuthLayout.blockSpacing) + block
            if used + needed > budget {
                column += 1
                if column >= columns { break }
                used = 0
                if block > budget { break }
                result[column].append(account)
                used = block
            } else {
                result[column].append(account)
                used += needed
            }
            shown += 1
        }
        return (result, shown)
    }

    @ViewBuilder
    private func medium(_ accounts: [WidgetAccount], snapshot: WidgetSnapshot, displayUsed: Bool) -> some View {
        let pending = accounts.filter(\.needsAction)
        let rest = pending.isEmpty ? accounts : accounts.filter { !$0.needsAction }
        header(accounts, snapshot: snapshot)
        if !pending.isEmpty {
            PendingAlert(accounts: pending)
        }
        // 按真实可用高度放：系统给中尺寸的内容区比预想的矮时少放一行，不会把最后一行裁掉一半
        GeometryReader { geo in
            let step = AuthLayout.compactRowHeight + AuthLayout.compactSpacing
            let fit = max(0, Int((geo.size.height + AuthLayout.compactSpacing) / step))
            VStack(alignment: .leading, spacing: AuthLayout.compactSpacing) {
                ForEach(Array(rest.prefix(fit)), id: \.uid) {
                    CompactAccountRow(account: $0, displayUsed: displayUsed, now: entry.date)
                }
            }
        }
    }

    private func footer(hidden: Int, snapshot: WidgetSnapshot) -> some View {
        HStack {
            if hidden > 0 {
                Text(String(format: Strings.current.more, hidden))
            }
            Spacer()
            UpdatedTime(snapshot: snapshot, now: entry.date)
        }
        .font(.system(size: 10).monospacedDigit())
        .foregroundStyle(.tertiary)
        .frame(height: AuthLayout.footerHeight)
    }
}

struct AccountsWidget: Widget {
    let kind = "com.ccswitch.desktop.widget.accounts"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: SnapshotProvider()) { entry in
            AccountsWidgetView(entry: entry)
                .widgetGlassContent(.panel)
                .containerBackground(for: .widget) { WidgetGlassBackground(style: .panel) }
        }
        .configurationDisplayName(widgetDisplayName(Strings.current.accountsTitle))
        .description(Strings.current.accountsGallery)
        .supportedFamilies([.systemMedium, .systemLarge, .systemExtraLarge])
    }
}
