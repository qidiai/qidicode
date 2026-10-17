@echo off
rem ============================================================
rem  qidicode 手机遥控 — 首尔反向隧道守护 (循环重连版)
rem  首尔 127.0.0.1:24190 → 本机 2419；断线 5 秒自动重连
rem ============================================================
:loop
ssh -N -R 127.0.0.1:24190:127.0.0.1:2419 -o ServerAliveInterval=30 -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes -o BatchMode=yes ubuntu@43.155.167.164
timeout /t 5 /nobreak >nul
goto loop
