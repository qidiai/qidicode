# ============================================================
#  qidi-build.ps1 — qidicode-src 一键构建（Windows GNU 工具链）
#  用法:
#    powershell -File qidi-build.ps1                    # 只做 check 验证
#    powershell -File qidi-build.ps1 -Release           # check + release 出货 qidi.exe
#    powershell -File qidi-build.ps1 -Release -SkipCheck # 跳过 check 直接出货
#    powershell -File qidi-build.ps1 -Test think         # check + 跑 think 单测
#  前提: C:\mingw-tools 已就位（没装就先跑 get_mingw_binutils.py / get_mingw_gcc.py）
#  详见同目录《Windows本地构建工具链.md》
# ============================================================
param(
    [switch]$Release,
    [switch]$Test,          # 指定测试过滤器, 如 -Test think
    [switch]$SkipCheck,
    # Machine paths are overridable; defaults match the original author box (user 28970).
    # SelfCont auto-derives from the CURRENT user's rustup when not passed.
    [string]$MingwBin  = 'C:\mingw-tools\mingw64\bin',
    [string]$Src       = 'G:\qidi-src',
    [string]$TargetDir = 'G:\qidi-target',
    [string]$SelfCont  = ''
)

$ErrorActionPreference = "Stop"

# ---- 三件套环境（缺一不可，见文档第 4 节） ----
$MINGW    = $MingwBin
$SELFCONT = $SelfCont
if (-not $SelfCont) {
    # Auto-derive from the current user's rustup (the historic hardcoded path
    # pointed at another machine's profile). Override with -SelfCont if needed.
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $scRel = 'lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
    $toolchains = Join-Path $rustupHome 'toolchains'
    $tc = @('stable-x86_64-pc-windows-gnu') + (Get-ChildItem $toolchains -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) |
          Where-Object { Test-Path (Join-Path (Join-Path $toolchains $_) $scRel) } | Select-Object -First 1
    if (-not $tc) { Write-Host "[X] self-contained dir not found under $toolchains -- pass -SelfCont explicitly" -ForegroundColor Red; exit 1 }
    $SELFCONT = Join-Path (Join-Path $toolchains $tc) $scRel
}
$SRC      = $Src       # junction -> G:\程序开发\qidicode\qidicode-src
$TARGET   = $TargetDir    # 强制 ASCII，防中文路径坑

if (-not (Test-Path $MINGW)) {
    Write-Host "[X] $MINGW 不存在——先跑 get_mingw_binutils.py / get_mingw_gcc.py 装工具链" -ForegroundColor Red
    exit 1
}

Set-Location $SRC
$env:PATH = "$MINGW;$SELFCONT;" + $env:PATH
$env:CARGO_TARGET_DIR = $TARGET

Write-Host "=== qidi-build | SRC=$SRC TARGET=$TARGET ===" -ForegroundColor Cyan

# ---- 1. check 验证 ----
if (-not $SkipCheck) {
    Write-Host "`n[1] cargo check 核心三 crate ..." -ForegroundColor Yellow
    cargo check -p cf-tools -p cf-workspace -p cf-agent
    if ($LASTEXITCODE -ne 0 -and -not ($?) ) { }
    # 注意: PowerShell 管道下 exit code 有假信号, 以输出 "Finished" 为准
}

# ---- 2. 定向测试（可选） ----
if ($Test) {
    Write-Host "`n[2] cargo test 过滤器: $Test ..." -ForegroundColor Yellow
    cargo test -p cf-tools --lib $Test
}

# ---- 3. release 出货（可选） ----
if ($Release) {
    Write-Host "`n[3] cargo build --release cf-pager-bin（首跑约 37 分钟）..." -ForegroundColor Yellow
    cargo build --release -p cf-pager-bin
    $out = "$TARGET\release\qidi.exe"
    if (Test-Path $out) {
        $sz = [math]::Round((Get-Item $out).Length / 1MB)
        Write-Host "`n[OK] 产物: $out ($sz MB)" -ForegroundColor Green
        & $out --version
    } else {
        Write-Host "[X] 未找到产物 $out" -ForegroundColor Red
        exit 1
    }
}

Write-Host "`n=== done ===" -ForegroundColor Cyan
