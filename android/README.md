# QIDI Code 遥控 · Android 壳工程

把 T1 交付的手机端（`../mobile`，ACP over WebSocket 客户端）包成一个可安装的
Android App：一个极简 WebView 壳，**不拷贝** `mobile/` 下的任何文件，而是让
Gradle 直接把 `../mobile` 作为 assets 源目录打包进 APK（单一事实源）。

- 应用 id / 命名空间：`com.qidicode.remote`
- 唯一 Activity：`MainActivity`（`WebView` + `WebViewAssetLoader`）
- 入口页面：`https://appassets.androidplatform.net/assets/index.html`
- minSdk 24（Android 7.0+），compileSdk / targetSdk 35

---

## 三种获取 APK 的路径

### ① 推到 GitHub，用 GitHub Actions 自动打包（推荐，无需本机环境）
推送到 `main` 分支且改动落在 `android/**` 或 `mobile/**` 时，workflow
`android-apk` 会自动构建 debug APK；也可在 Actions 页面手动触发
（`workflow_dispatch`）。
构建完成后在对应 run 的 **Artifacts** 里下载 `qidicode-remote-apk`
（其中是重命名后的 `qidicode-remote-debug.apk`）。

> ⚠️ **醒目提示：Gitee 不执行 `.github/workflows`。**
> 本仓库若托管在 Gitee，Actions 不会运行。要走这条路，必须把仓库同步/镜像到
> GitHub（例如用 Gitee 的「仓库镜像」或手动 push 到 GitHub 仓库），再在 GitHub 上
> 触发 workflow。否则请改用路径 ② 或 ③。

### ② 用 Android Studio 打开 `android/` 目录直接构建
用 Android Studio 打开本目录（`android/`），等待 Gradle 同步完成后：
`Build → Build App Bundle(s) / APK(s) → Build APK(s)`。
产物位于 `app/build/outputs/apk/debug/app-debug.apk`。
Android Studio 自带 Android SDK 与 Gradle，无需额外准备。

### ③ 命令行构建（需自备 JDK 17 + Android SDK）
```bash
# 需要：JDK 17、Android SDK（并设置 ANDROID_HOME / ANDROID_SDK_ROOT），
# 以及一个可用的 Gradle（本仓库不提交 gradle wrapper 二进制）。
gradle -p android assembleDebug --no-daemon
# 产物：android/app/build/outputs/apk/debug/app-debug.apk
```

---

## 安装说明
1. 把 APK 传到手机。
2. 在系统设置里允许「未知来源 / 安装未知应用」（Android 8+ 需针对具体来源 App 授权）。
3. 直接安装即可。debug 构建自带 debug 签名，可直接安装运行。

---

## 已知限制
- **明文 `ws://`**：页面需通过明文 WebSocket 连接电脑端，因此
  `usesCleartextTraffic="true"` 且允许混合内容。**仅建议在可信局域网内使用**，
  不要在不可信网络下连接。
- **debug 签名**：产物使用 debug 签名，**不适合公开分发**（上架应用商店需正式签名）。
- **系统版本**：最低 Android 7.0（minSdk 24）。

---

## 目录结构
```
android/
├── settings.gradle.kts       # 单模块 :app
├── build.gradle.kts          # 插件版本（AGP 8.7.3 / Kotlin 2.0.21）
├── gradle.properties         # jvmargs / AndroidX 开关
└── app/
    ├── build.gradle.kts      # 模块配置 + assets 引用 ../mobile
    ├── proguard-rules.pro
    └── src/main/
        ├── AndroidManifest.xml
        ├── java/com/qidicode/remote/MainActivity.kt
        └── res/
            ├── values/{strings.xml,themes.xml,colors.xml}
            ├── drawable/ic_launcher_foreground.xml
            └── mipmap-anydpi-v26/ic_launcher.xml
```

> 注：本仓库不提交 gradle wrapper 二进制（`gradle-wrapper.jar` 等）。
> CI 通过 `gradle/actions/setup-gradle` 指定 Gradle 版本；Android Studio 自带。
