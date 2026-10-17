@echo off
rem ============================================================
rem  qidicode mobile remote - watchdog (Task Scheduler every 30 min)
rem  Logic: GET /healthz fails -> relaunch component (idempotent)
rem  NOTE: /healthz (HTTP 200 + JSON) is a stronger liveness signal
rem  than a raw LISTENING port probe - it proves axum is serving.
rem ============================================================
curl.exe -sf -m 5 http://127.0.0.1:2419/healthz >nul 2>&1
if errorlevel 1 (
    schtasks /run /tn "QidiServe" >nul 2>&1
)
ssh -o BatchMode=yes -o ConnectTimeout=6 ubuntu@43.155.167.164 "ss -tln | grep -q 24190" >nul 2>&1
if errorlevel 1 (
    schtasks /run /tn "QidiTunnel" >nul 2>&1
)
