# QIDI Code 遥控：ProGuard/R8 规则
# 说明：release 与 debug 均设置 isMinifyEnabled = false，
# 本文件当前无需额外规则，仅保留占位以便日后启用混淆时使用。
#
# 若日后启用混淆（isMinifyEnabled = true），需保留 WebView 注入接口相关类，
# 例如使用 @JavascriptInterface 注解的方法不被移除。
