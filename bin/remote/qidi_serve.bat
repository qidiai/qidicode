@echo off
rem ============================================================
rem  qidicode 手机遥控 — serve 守护 (Windows 计划任务部署模板)
rem  部署见同目录 README.md；密钥文件绝不入库
rem ============================================================
rem  ⚠️ < NUL 是命门：计划任务/独立环境下 qidi serve 会阻塞在
rem  控制台 stdin 上（永远绑不上端口），必须把 stdin 指向 NUL。
if not exist "%USERPROFILE%\.qidi\remote_secret.txt" (
    echo [ERROR] missing %USERPROFILE%\.qidi\remote_secret.txt
    exit /b 1
)
set /p QIDI_SECRET=<"%USERPROFILE%\.qidi\remote_secret.txt"
rem %~dp0 = bin\remote\，回退两级到仓库根（qidi.exe 所在目录）
cd /d "%~dp0..\.."
qidi.exe agent --yolo-tools read_file,grep serve --bind 0.0.0.0:2419 --secret %QIDI_SECRET% < NUL
