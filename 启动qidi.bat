@echo off
rem QIDI Code portable launcher
rem Use UTF-8 codepage and declare color-capable TERM so the TUI renders CJK correctly.

rem If launched by double-click (no parent terminal), relaunch inside Windows Terminal for better resize support
rem wt.exe only exists on Windows 10 1903+ / Windows 11 with Windows Terminal installed
where wt.exe >nul 2>&1 && (
    rem Check if we are already inside Windows Terminal (WT_SESSION env var is set)
    if not defined WT_SESSION (
        for %%F in ("%~dp0.") do set "QIDI_DIR=%%~fF"
        wt.exe -d "%QIDI_DIR%" cmd /c "%~f0" %*
        exit /b
    )
)

chcp 65001 >nul
set "TERM=xterm-256color"
set "QIDI_HOME=%~dp0.qidi"

rem Model auto-sync: fetch model list from the local relay's /v1/models
rem (relay is keyless on LAN; this key is just a placeholder for the Bearer header)
set "XAI_API_KEY=lan-no-key"

rem Local services self-heal: QiDi Relay (8765, upstream) + websearch shim (8766).
rem WMI-spawned processes survive any launcher context; starts whichever is down.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0.qidi\ensure_services.ps1" >nul 2>&1

"%~dp0qidi.exe" %*
