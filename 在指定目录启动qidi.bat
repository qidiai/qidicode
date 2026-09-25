@echo off
setlocal
chcp 65001 >nul
rem Launch QIDI Code with the workspace set to a chosen project folder.
rem Usage 1: drag a folder onto this file
rem Usage 2: run  "在指定目录启动qidi.bat" "D:\your\project"
rem Usage 3: double-click, a folder picker will pop up

set "TERM=xterm-256color"
set "QIDI_HOME=%~dp0.qidi"

rem Model auto-sync: fetch model list from the local relay's /v1/models
rem (relay is keyless on LAN; this key is just a placeholder for the Bearer header)
set "XAI_API_KEY=lan-no-key"

set "TARGET=%~1"

if not defined TARGET (
    for /f "usebackq delims=" %%D in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0.qidi\ops\pick_folder.ps1"`) do set "TARGET=%%D"
)

if not defined TARGET (
    echo [Cancelled] No folder selected.
    pause
    exit /b 1
)

if not exist "%TARGET%\" (
    echo [Error] Folder not found: %TARGET%
    pause
    exit /b 1
)

cd /d "%TARGET%"
echo Starting QIDI Code in %CD% ...
"%~dp0qidi.exe"