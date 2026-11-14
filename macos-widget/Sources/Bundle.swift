// 扩展入口。预览程序（preview.sh / glass-preview.sh）定义 CC_SWITCH_WIDGET_PREVIEW，自带入口。

import SwiftUI
import WidgetKit

#if !CC_SWITCH_WIDGET_PREVIEW
@main
struct CCSwitchWidgetBundle: WidgetBundle {
    var body: some Widget {
        QuotaWidget()
        AccountsWidget()
        UsageWidget()
    }
}
#endif
