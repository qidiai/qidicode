plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.qidicode.remote"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.qidicode.remote"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            // 不混淆/不删减（壳工程体量小，保持可调试可读）
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // 关键：APK 内 assets 直接引用仓库中的 mobile/ 目录（不拷贝，单一事实源）。
    // 打包后 APK 内的 assets/ 即为 mobile/ 下的文件（index.html / app.js / style.css / README.md）。
    // 注意：srcDir 相对路径以模块目录（android/app/）为基准，故必须用 rootProject 定位仓库根。
    sourceSets {
        getByName("main") {
            assets.srcDir(rootProject.file("../mobile"))
        }
    }
}

dependencies {
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.webkit:webkit:1.12.1")
    implementation("androidx.activity:activity-ktx:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
}
