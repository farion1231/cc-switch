// 小组件自己的文案（快照里的文字主程序已按界面语言写好，这里只有标题、提示）。按系统语言取。

import Foundation

struct Strings {
    let quotaTitle: String
    let quotaGallery: String
    let accountsTitle: String
    let accountsGallery: String
    let usageTitle: String
    let usageGallery: String
    let off: String
    let outdated: String
    let noQuota: String
    let noAccounts: String
    let inUse: String
    let reauth: String
    let today: String
    let last7Days: String
    let last30Days: String
    let tokens: String
    let requests: String
    let cacheHit: String
    /// "%d 个账号"
    let accountCount: String
    /// "还有 %d 个"
    let more: String
    /// "%d 个要处理"
    let needAction: String
    /// 快照太久没更新（CC Switch 多半没开着）
    let stale: String
    let cost: String
    let noUsage: String
    /// "还有 %d 档"（一个号放不下的额度档）
    let moreTiers: String
    /// 新版主程序刚启动、账号还没读到
    let loadingAccounts: String

    /// 「N 个账号」；英文只有 1 个时用单数
    func accounts(_ count: Int) -> String {
        if count == 1, accountCount == "%d accounts" { return "1 account" }
        return String(format: accountCount, count)
    }

    static let current: Strings = {
        let lang = Locale.preferredLanguages.first?.lowercased() ?? "en"
        if lang.hasPrefix("zh-hant") || lang.hasPrefix("zh-tw") || lang.hasPrefix("zh-hk") {
            return Strings(
                quotaTitle: "額度", quotaGallery: "各應用使用中的供應商與額度。",
                accountsTitle: "授權中心", accountsGallery: "授權中心所有帳號的額度，快用完和需要重新登入的排在前面。",
                usageTitle: "用量", usageGallery: "花費、Token 與請求數；時間範圍與樣式可在「編輯小工具」裡選。",
                off: "在 CC Switch 設定裡開啟「桌面小工具」", outdated: "請更新 CC Switch",
                noQuota: "沒有額度資料", noAccounts: "授權中心還沒有帳號",
                inUse: "使用中", reauth: "需重新登入",
                today: "今天", last7Days: "近 7 天", last30Days: "近 30 天",
                tokens: "Token", requests: "請求", cacheHit: "快取命中",
                accountCount: "%d 個帳號", more: "還有 %d 個", needAction: "%d 個待處理",
                stale: "資料已過時", cost: "花費", noUsage: "這段時間沒有用量", moreTiers: "還有 %d 檔", loadingAccounts: "正在讀取帳號…"
            )
        }
        if lang.hasPrefix("zh") {
            return Strings(
                quotaTitle: "额度", quotaGallery: "各应用在用的供应商和额度。",
                accountsTitle: "授权中心", accountsGallery: "授权中心全部账号的额度，快用完和要重新登录的排在前面。",
                usageTitle: "用量", usageGallery: "花费、Token 和请求数；时间范围和样式可在「编辑小组件」里选。",
                off: "在 CC Switch 设置里打开「桌面小组件」", outdated: "请更新 CC Switch",
                noQuota: "没有额度数据", noAccounts: "授权中心还没有账号",
                inUse: "在用", reauth: "要重新登录",
                today: "今天", last7Days: "近 7 天", last30Days: "近 30 天",
                tokens: "Token", requests: "请求", cacheHit: "缓存命中",
                accountCount: "%d 个账号", more: "还有 %d 个", needAction: "%d 个要处理",
                stale: "数据已过时", cost: "花费", noUsage: "这段时间没有用量", moreTiers: "还有 %d 档", loadingAccounts: "正在读取账号…"
            )
        }
        if lang.hasPrefix("tr") {
            return Strings(
                quotaTitle: "Kota", quotaGallery: "Her uygulamanın etkin sağlayıcısı ve kotası.",
                accountsTitle: "Hesaplar", accountsGallery: "Tüm hesapların kotası; azalanlar ve yeniden giriş gerekenler önce gösterilir.",
                usageTitle: "Kullanım", usageGallery: "Maliyet, token ve istekler. Zaman aralığını ve stili Widget'ı Düzenle'den seçin.",
                off: "CC Switch ayarlarından Masaüstü widget'ını açın", outdated: "CC Switch'i güncelleyin",
                noQuota: "Kota bilgisi yok", noAccounts: "Henüz hesap yok",
                inUse: "Kullanımda", reauth: "Yeniden giriş yapın",
                today: "Bugün", last7Days: "Son 7 gün", last30Days: "Son 30 gün",
                tokens: "Token", requests: "İstek", cacheHit: "Önbellek isabeti",
                accountCount: "%d hesap", more: "%d tane daha", needAction: "%d tanesi ilgi bekliyor",
                stale: "Veriler eski", cost: "Maliyet", noUsage: "Bu aralıkta kullanım yok", moreTiers: "%d tane daha",
                loadingAccounts: "Hesaplar yükleniyor…"
            )
        }
        if lang.hasPrefix("ja") {
            return Strings(
                quotaTitle: "クォータ", quotaGallery: "各アプリで使用中のプロバイダーとクォータ。",
                accountsTitle: "認証センター", accountsGallery: "認証センターの全アカウントのクォータ。残りわずか・再ログインが必要なものを先に表示します。",
                usageTitle: "使用量", usageGallery: "費用、トークン、リクエスト数。期間とスタイルは「ウィジェットを編集」で選べます。",
                off: "CC Switch の設定で「デスクトップウィジェット」をオンにしてください", outdated: "CC Switch を更新してください",
                noQuota: "クォータ情報なし", noAccounts: "認証センターにアカウントがありません",
                inUse: "使用中", reauth: "再ログインが必要",
                today: "今日", last7Days: "過去 7 日", last30Days: "過去 30 日",
                tokens: "トークン", requests: "リクエスト", cacheHit: "キャッシュヒット",
                accountCount: "%d 件のアカウント", more: "ほか %d 件", needAction: "%d 件要対応",
                stale: "データが古い", cost: "費用", noUsage: "この期間の使用量はありません", moreTiers: "ほか %d 件", loadingAccounts: "アカウントを読み込み中…"
            )
        }
        return Strings(
            quotaTitle: "Quota", quotaGallery: "Each app's in-use provider and quota.",
            accountsTitle: "Auth Center", accountsGallery: "Quota for every Auth Center account, with low and signed-out ones first.",
            usageTitle: "Usage", usageGallery: "Cost, tokens and requests. Pick the time range and style in Edit Widget.",
            off: "Turn on Desktop widget in CC Switch settings", outdated: "Update CC Switch",
            noQuota: "No quota data", noAccounts: "No Auth Center accounts yet",
            inUse: "In use", reauth: "Sign in again",
            today: "Today", last7Days: "Last 7 days", last30Days: "Last 30 days",
            tokens: "Tokens", requests: "Requests", cacheHit: "Cache hits",
            accountCount: "%d accounts", more: "%d more", needAction: "%d need attention",
            stale: "Outdated", cost: "Cost", noUsage: "No usage in this range", moreTiers: "%d more", loadingAccounts: "Loading accounts…"
        )
    }()
}
