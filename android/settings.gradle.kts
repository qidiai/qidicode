// QIDI Code 遥控 —— Android WebView 壳工程
// 单模块 :app，插件与依赖仓库集中在此声明。
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "qidicode-remote"
include(":app")
