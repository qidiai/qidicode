# QIDI Code release 一键编译脚本
# 用法：在 PowerShell 里执行  .\build_release.ps1
# 编译完成后结果写入 build_result.txt，日志写入 build_log.txt

Set-Location $PSScriptRoot

Write-Host "======================================"
Write-Host " 开始编译 release 版 qidi"
Write-Host " 预计 40~90 分钟，请耐心等待..."
Write-Host " 中途请勿关闭此窗口"
Write-Host "======================================"

cargo build --release -j 2 *> build_log.txt

if ($LASTEXITCODE -eq 0) {
    Set-Content -Path build_result.txt -Value "OK - 编译成功"
    Write-Host ""
    Write-Host "======================================"
    Write-Host " 编译成功！"
    Write-Host " 新版程序在: target\release\qidi.exe"
    Write-Host " 运行命令:   .\target\release\qidi.exe"
    Write-Host "======================================"
    [console]::beep(1000, 300)
} else {
    Set-Content -Path build_result.txt -Value "FAIL - 编译失败，请看 build_log.txt"
    Write-Host ""
    Write-Host "======================================"
    Write-Host " 编译失败了，错误信息在 build_log.txt"
    Write-Host " 如果报「页面文件太小」，把脚本里 -j 2 改成 -j 1 再跑一次"
    Write-Host "======================================"
}
