// 顶层构建脚本：只声明插件版本，apply false，由子模块按需应用。
// AGP 8.7.3 + Kotlin 2.0.21（规格钉死，不得更换）。
plugins {
    id("com.android.application") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
