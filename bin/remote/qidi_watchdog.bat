@echo off
rem ============================================================
rem  qidicode 手机遥控 — 看门狗 (Task Scheduler 每 30 分钟巡检)
rem  逻辑：端口不在 → 用计划任务拉起对应组件（幂等，服务健康时零动作）
rem ============================================================
netstat -an | findstr /c:":2419 " | findstr /c:"LISTENING" >nul 2>&1
if errorlevel 1 (
    schtasks /run /tn "QidiServe" >nul 2>&1
)
ssh -o BatchMode=yes -o ConnectTimeout=6 ubuntu@43.155.167.164 "ss -tln | grep -q 24190" >nul 2>&1
if errorlevel 1 (
    schtasks /run /tn "QidiTunnel" >nul 2>&1
)
