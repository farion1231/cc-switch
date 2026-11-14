// 主程序（Rust）通知桌面小组件重新读快照用。WidgetCenter 只有 Swift 接口，
// 由 build.rs 编成静态库链进 cc-switch；Rust 侧见 services/widget_snapshot/reload.rs。
import WidgetKit

@_cdecl("ccswitch_reload_widget_timelines")
public func ccswitchReloadWidgetTimelines() {
    WidgetCenter.shared.reloadAllTimelines()
}
