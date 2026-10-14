package com.qidicode.remote

import android.annotation.SuppressLint
import android.os.Bundle
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import androidx.webkit.WebResourceErrorCompat
import androidx.webkit.WebViewAssetLoader
import androidx.webkit.WebViewClientCompat

/**
 * 唯一 Activity：极简 WebView 壳。
 *
 * 界面来自 APK 内置 assets（即仓库 mobile/ 目录，通过 sourceSets 直接引用）。
 * 资源经 WebViewAssetLoader 映射到合成安全源
 * https://appassets.androidplatform.net/assets/ ，使页面处于 https 安全上下文，
 * 从而允许页面内对 ws:// 明文 WebSocket 的连接（仅限可信局域网使用）。
 */
class MainActivity : AppCompatActivity() {

    private lateinit var webView: WebView

    // 二次返回退出：上一次按下返回键的时间戳（毫秒）
    private var lastBackPress = 0L

    // assets 加载器：把 /assets/ 路径映射到 APK 内置 assets。
    private val assetLoader: WebViewAssetLoader by lazy {
        WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(this))
            .build()
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 不使用 XML 布局，代码内直接创建 WebView
        webView = WebView(this)
        setContentView(webView)

        // 页面运行在 https 合成源上，需要允许其加载 ws:// 明文连接
        // （MIXED_CONTENT_ALWAYS_ALLOW）；localStorage 依赖 domStorageEnabled。
        webView.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            mixedContentMode = WebSettings.MIXED_CONTENT_ALWAYS_ALLOW
            allowFileAccess = false
        }

        webView.webViewClient = object : WebViewClientCompat() {
            // 标准写法：所有请求交给 assetLoader，从内置 assets 读取本地文件。
            override fun shouldInterceptRequest(
                view: WebView,
                request: WebResourceRequest
            ): WebResourceResponse? {
                return assetLoader.shouldInterceptRequest(request.url)
            }

            // 主框架加载失败：在 WebView 内显示错误文案 HTML（inline 字符串，不依赖资源）。
            override fun onReceivedError(
                view: WebView,
                request: WebResourceRequest,
                error: WebResourceErrorCompat
            ) {
                if (request.isForMainFrame) {
                    val detail = error.description?.toString() ?: "未知错误"
                    view.loadDataWithBaseURL(
                        BASE_URL,
                        errorHtml(detail),
                        "text/html",
                        "utf-8",
                        null
                    )
                }
            }
        }

        // 返回键：能后退则后退；否则 2 秒内再按一次才退出，否则 Toast 提示。
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (webView.canGoBack()) {
                    webView.goBack()
                    return
                }
                val now = System.currentTimeMillis()
                if (now - lastBackPress < 2000) {
                    finish()
                } else {
                    lastBackPress = now
                    Toast.makeText(this@MainActivity, "再按一次退出", Toast.LENGTH_SHORT).show()
                }
            }
        })

        webView.loadUrl(INDEX_URL)
    }

    override fun onPause() {
        webView.onPause()
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        webView.onResume()
    }

    override fun onDestroy() {
        // 先卸载页面再销毁，避免 WebView 泄漏
        webView.loadUrl("about:blank")
        webView.destroy()
        super.onDestroy()
    }

    // 错误页 HTML（inline 字符串，深色配色与 App 保持一致）
    private fun errorHtml(detail: String): String {
        return """
            <!DOCTYPE html>
            <html lang="zh-CN"><head><meta charset="utf-8">
            <meta name="viewport" content="width=device-width,initial-scale=1">
            <style>
            body{background:#1e1f24;color:#e6e6e6;font-family:sans-serif;
            padding:2rem;line-height:1.6}
            h1{font-size:1.2rem}code{color:#9cdcfe;word-break:break-all}
            </style></head><body>
            <h1>页面加载失败</h1>
            <p>无法从内置资源加载界面，请重新安装应用后重试。</p>
            <p><code>$detail</code></p>
            </body></html>
        """.trimIndent()
    }

    companion object {
        // WebViewAssetLoader 提供的合成源基址
        private const val BASE_URL = "https://appassets.androidplatform.net/assets/"
        // 入口页面
        private const val INDEX_URL = "https://appassets.androidplatform.net/assets/index.html"
    }
}
