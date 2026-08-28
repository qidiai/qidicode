# archive.ps1 - qidicode-portable 归档规则固化（Scan / ArchiveRoot / Slim 三模式运维脚本）
#
# 背景：工作区采用 _archive/<日期>/ 归档制（只进不出），归档全靠手工，归档区会无限膨胀。
#       本脚本把归档规则固化为可复用命令，默认 dry-run，加 -Execute 才真操作（零删零移护栏）。
#
# 三模式：
#   Scan        （默认，只读）列出根目录可归档项（内置清单）+ 扫描 _archive/ 内可再生物，
#               汇总"若执行可回收 MB / 将移动 N 项"。
#   ArchiveRoot 把根目录内置清单项归档到 _archive/<今日>/（文件->root_docs\，目录整移）。
#   Slim        只作用于 _archive/ 内部：删 node_modules / __pycache__ / 散落 *.pyc；
#               qidi.exe.* 备份仅当总数>2 时删最老（保留最新 2）。永不碰根目录与 .qidi。
#
# 长路径防御：_archive 含 URL 编码超长路径(>260 字符，历史报 DirectoryNotFound)，
#             遍历与删除一律 -LiteralPath；遇异常 WARN 跳过不中断。
#
# 编码：本文件含中文，必须存成 UTF-8 带 BOM（PS5.1 无 BOM 按 GBK 解析报"字符串缺少终止符"）。
#
# 用法示例：
#   powershell -NoProfile -ExecutionPolicy Bypass -File .qidi\ops\archive.ps1                # Scan（只读）
#   powershell -NoProfile -ExecutionPolicy Bypass -File .qidi\ops\archive.ps1 -Mode Slim     # Slim dry-run
#   powershell -NoProfile -ExecutionPolicy Bypass -File .qidi\ops\archive.ps1 -Mode ArchiveRoot -Execute

param(
    [ValidateSet('Scan','ArchiveRoot','Slim')] [string]$Mode = 'Scan',
    [string]$ArchiveDir = '',          # 默认 _archive/<今日yyyy-MM-dd>
    [switch]$Execute                   # 不带此开关一律 dry-run
)

$ErrorActionPreference = "Continue"

# ---- 工作区根定位：脚本位于 <root>\.qidi\ops\，根 = 上两级（不依赖 cwd）----
$scriptRoot  = $PSScriptRoot
$qidiHome    = (Resolve-Path (Join-Path $scriptRoot "..")).Path   # <root>\.qidi
$root        = Split-Path -Parent $qidiHome                        # <root>
$archiveBase = Join-Path $root "_archive"

# ---- 根目录内置归档清单（硬编码在脚本顶部）----
$RootFiles = @(
    '修复与剥离报告_方案模块_20260827.md',
    '审计报告_标书软件_20260827.md',
    '审计验证报告_最终版.md',
    'image8_check.png',
    'img1_org.png',
    'img2_site.png',
    'img3.png',
    'img4.png',
    'img5.png',
    'img6_net.png',
    'img7_gantt.png',
    'img8.png'
)
$RootDirs = @(
    '_audit_fix',
    'bid_work',
    '_external_read_tmp'
)

# ---- 汇总计数器 ----
$script:moved   = 0
$script:deleted = 0
$script:skipped = 0
$script:reclaimedBytes = [int64]0
$script:warnings = @()

$dryRun = -not $Execute

# ---- 工具函数 ----
function Show-Relative([string]$full, [string]$base) {
    if ($full.Length -gt $base.Length) { return $full.Substring($base.Length).TrimStart('\','/') }
    return $full
}

function Write-Action([string]$verb, [string]$detail) {
    if ($dryRun) { Write-Output ("[OK] would {0} {1}" -f $verb, $detail) }
    else         { Write-Output ("[OK] {0}d {1}" -f $verb, $detail) }
}

# 目录整体字节数（-LiteralPath BFS，长路径目录静默跳过不影响统计）
function Get-DirSize([string]$path) {
    $sum = [int64]0
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($path)
    while ($queue.Count -gt 0) {
        $d = [string]$queue.Dequeue()
        try {
            foreach ($f in @(Get-ChildItem -LiteralPath $d -File -ErrorAction Stop)) {
                $sum += [int64]$f.Length
            }
            foreach ($sub in @(Get-ChildItem -LiteralPath $d -Directory -ErrorAction Stop)) {
                $queue.Enqueue($sub.FullName)
            }
        } catch { }
    }
    return $sum
}

# 单次 -LiteralPath BFS 遍历 _archive：收集可再生物清单（防递归栈深；长路径异常收集 WARN 跳过不中断）
function Get-ArchiveInventory {
    $nmDirs = @(); $pcDirs = @(); $pycAll = @(); $exeAll = @()
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($archiveBase)
    while ($queue.Count -gt 0) {
        $dir = [string]$queue.Dequeue()
        try {
            foreach ($sub in @(Get-ChildItem -LiteralPath $dir -Directory -ErrorAction Stop)) {
                $leaf = Split-Path -Leaf $sub.FullName
                if ($leaf -eq 'node_modules')    { $nmDirs += $sub.FullName }
                elseif ($leaf -eq '__pycache__') { $pcDirs += $sub.FullName }
                $queue.Enqueue($sub.FullName)
            }
            foreach ($f in @(Get-ChildItem -LiteralPath $dir -File -ErrorAction Stop)) {
                if ($f.Name -like '*.pyc')           { $pycAll += $f }
                elseif ($f.Name -like 'qidi.exe.*')  { $exeAll += $f }
            }
        } catch {
            $script:warnings += ("  [WARN] 扫描跳过(超长路径/无权限): {0}" -f $dir)
            $script:skipped++
        }
    }
    $scatteredPyc = @($pycAll | Where-Object { (Split-Path -Leaf $_.DirectoryName) -ne '__pycache__' })
    $exeSorted = @($exeAll | Sort-Object LastWriteTime -Descending)
    $exeToDelete = @()
    if ($exeSorted.Count -gt 2) { $exeToDelete = @($exeSorted | Select-Object -Skip 2) }

    $nmBytes = [int64]0; foreach ($d in $nmDirs) { $nmBytes += Get-DirSize $d }
    $pcBytes = [int64]0; foreach ($d in $pcDirs) { $pcBytes += Get-DirSize $d }
    $pycBytes = [int64]0; foreach ($f in $scatteredPyc) { $pycBytes += [int64]$f.Length }
    $exeBytes = [int64]0; foreach ($f in $exeToDelete) { $exeBytes += [int64]$f.Length }

    return [PSCustomObject]@{
        nmDirs = $nmDirs; pcDirs = $pcDirs; scatteredPyc = $scatteredPyc
        exeAll = $exeAll; exeToDelete = $exeToDelete
        nmBytes = $nmBytes; pcBytes = $pcBytes; pycBytes = $pycBytes; exeBytes = $exeBytes
    }
}

# ---- Scan（只读，默认）----
function Invoke-Scan {
    Write-Output ""
    Write-Output ("==== 1) 根目录可归档项（内置清单，共 {0} 文件 + {1} 目录）====" -f $RootFiles.Count, $RootDirs.Count)
    $nFiles = 0; $nDirs = 0
    foreach ($f in $RootFiles) {
        $src = Join-Path $root $f
        if (Test-Path -LiteralPath $src) {
            $nFiles++
            Write-Output ("  [OK] would move {0} -> _archive\<今日>\root_docs\" -f $f)
        } else {
            Write-Output ("  [--] skip (not found): {0}" -f $f)
            $script:skipped++
        }
    }
    foreach ($d in $RootDirs) {
        $src = Join-Path $root $d
        if (Test-Path -LiteralPath $src) {
            $nDirs++
            Write-Output ("  [OK] would move dir {0} -> _archive\<今日>\" -f $d)
        } else {
            Write-Output ("  [--] skip (not found): {0}" -f $d)
            $script:skipped++
        }
    }
    $script:moved = $nFiles + $nDirs
    Write-Output ("根目录可归档项: {0} 个（{1} 文件 + {2} 目录）" -f ($nFiles + $nDirs), $nFiles, $nDirs)

    Write-Output ""
    Write-Output "==== 2) 扫描 _archive/ 内可再生物（Slim 目标）===="
    $inv = Get-ArchiveInventory
    Write-Output ("  node_modules: {0} 个（约 {1} MB）" -f $inv.nmDirs.Count, [math]::Round($inv.nmBytes / 1MB, 1))
    Write-Output ("  __pycache__:  {0} 个（约 {1} MB）" -f $inv.pcDirs.Count, [math]::Round($inv.pcBytes / 1MB, 1))
    Write-Output ("  散落 *.pyc:   {0} 个（约 {1} MB）" -f $inv.scatteredPyc.Count, [math]::Round($inv.pycBytes / 1MB, 1))
    Write-Output ("  qidi.exe.* 备份 {0} 个（保留最新 2，将删最老 {1} 个，约 {2} MB）" -f $inv.exeAll.Count, $inv.exeToDelete.Count, [math]::Round($inv.exeBytes / 1MB, 1))
    foreach ($d in $inv.nmDirs)     { Write-Output ("    [OK] would delete node_modules {0}" -f (Show-Relative $d $archiveBase)) }
    foreach ($d in $inv.pcDirs)     { Write-Output ("    [OK] would delete __pycache__ {0}" -f (Show-Relative $d $archiveBase)) }
    foreach ($f in $inv.scatteredPyc) { Write-Output ("    [OK] would delete {0}" -f (Show-Relative $f.FullName $archiveBase)) }
    foreach ($f in $inv.exeToDelete)  { Write-Output ("    [OK] would delete {0}" -f (Show-Relative $f.FullName $archiveBase)) }

    $script:deleted = $inv.nmDirs.Count + $inv.pcDirs.Count + $inv.scatteredPyc.Count + $inv.exeToDelete.Count
    $script:reclaimedBytes = $inv.nmBytes + $inv.pcBytes + $inv.pycBytes + $inv.exeBytes
}

# ---- ArchiveRoot：把根目录内置清单项归档到 $ArchiveDir ----
function Invoke-ArchiveRoot {
    if ($ArchiveDir -eq '') {
        $dest = Join-Path $archiveBase (Get-Date -Format 'yyyy-MM-dd')
    } elseif ([System.IO.Path]::IsPathRooted($ArchiveDir)) {
        $dest = $ArchiveDir
    } else {
        $dest = Join-Path $root $ArchiveDir
    }
    $destDocs = Join-Path $dest 'root_docs'
    Write-Output ""
    Write-Output ("==== ArchiveRoot：根目录内置清单 -> {0} ====" -f (Show-Relative $dest $root))
    if (-not $dryRun) { New-Item -ItemType Directory -Path $destDocs -Force | Out-Null }

    foreach ($f in $RootFiles) {
        $src = Join-Path $root $f
        if (Test-Path -LiteralPath $src) {
            Write-Action "move" ("{0} -> {1}\" -f $f, (Show-Relative $destDocs $root))
            if ($dryRun) { $script:moved++ }
            else {
                try { Move-Item -LiteralPath $src -Destination $destDocs -Force; $script:moved++ }
                catch { Write-Output ("  [WARN] move fail: {0}" -f $f); $script:skipped++ }
            }
        } else {
            Write-Output ("  [--] skip (not found): {0}" -f $f)
            $script:skipped++
        }
    }
    foreach ($d in $RootDirs) {
        $src = Join-Path $root $d
        if (Test-Path -LiteralPath $src) {
            Write-Action "move" ("dir {0} -> {1}\" -f $d, (Show-Relative $dest $root))
            if ($dryRun) { $script:moved++ }
            else {
                try { Move-Item -LiteralPath $src -Destination $dest -Force; $script:moved++ }
                catch { Write-Output ("  [WARN] move fail: {0}" -f $d); $script:skipped++ }
            }
        } else {
            Write-Output ("  [--] skip (not found): {0}" -f $d)
            $script:skipped++
        }
    }
}

# ---- Slim：只作用于 _archive 内部（永不碰根目录与 .qidi）----
function Invoke-Slim {
    Write-Output ""
    Write-Output "==== Slim：只作用于 _archive/ 内部（不碰根目录与 .qidi）===="
    $inv = Get-ArchiveInventory

    foreach ($d in $inv.nmDirs) {
        $bytes = Get-DirSize $d
        Write-Action "delete" ("node_modules {0}" -f (Show-Relative $d $archiveBase))
        if ($dryRun) { $script:deleted++ }
        else {
            try { Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction Stop; $script:deleted++ }
            catch { Write-Output ("  [WARN] delete fail: {0}" -f $d); $script:skipped++ }
        }
        $script:reclaimedBytes += $bytes
    }
    foreach ($d in $inv.pcDirs) {
        $bytes = Get-DirSize $d
        Write-Action "delete" ("__pycache__ {0}" -f (Show-Relative $d $archiveBase))
        if ($dryRun) { $script:deleted++ }
        else {
            try { Remove-Item -LiteralPath $d -Recurse -Force -ErrorAction Stop; $script:deleted++ }
            catch { Write-Output ("  [WARN] delete fail: {0}" -f $d); $script:skipped++ }
        }
        $script:reclaimedBytes += $bytes
    }
    foreach ($f in $inv.scatteredPyc) {
        Write-Action "delete" ("{0}" -f (Show-Relative $f.FullName $archiveBase))
        if ($dryRun) { $script:deleted++ }
        else {
            try { Remove-Item -LiteralPath $f.FullName -Force -ErrorAction Stop; $script:deleted++ }
            catch { Write-Output ("  [WARN] delete fail: {0}" -f $f.FullName); $script:skipped++ }
        }
        $script:reclaimedBytes += [int64]$f.Length
    }
    if ($inv.exeToDelete.Count -gt 0) {
        Write-Output ("  qidi.exe.* 备份 {0} 个 > 2，删最老 {1} 个（保留最新 2）" -f $inv.exeAll.Count, $inv.exeToDelete.Count)
        foreach ($f in $inv.exeToDelete) {
            Write-Action "delete" ("{0}（约 {1} MB）" -f (Show-Relative $f.FullName $archiveBase), [math]::Round($f.Length / 1MB, 1))
            if ($dryRun) { $script:deleted++ }
            else {
                try { Remove-Item -LiteralPath $f.FullName -Force -ErrorAction Stop; $script:deleted++ }
                catch { Write-Output ("  [WARN] delete fail: {0}" -f $f.FullName); $script:skipped++ }
            }
            $script:reclaimedBytes += [int64]$f.Length
        }
    } else {
        Write-Output ("  qidi.exe.* 备份 {0} 个 <= 2，全部保留，不删" -f $inv.exeAll.Count)
    }
}

# ---- 主流程 ----
Write-Output ("archive.ps1 Mode={0} dry-run={1}" -f $Mode, $dryRun)
Write-Output ("Root = {0}" -f $root)

switch ($Mode) {
    'Scan'        { Invoke-Scan }
    'ArchiveRoot' { Invoke-ArchiveRoot }
    'Slim'        { Invoke-Slim }
}

foreach ($w in $script:warnings) { Write-Output $w }

Write-Output ""
Write-Output ("==== 汇总：移动 {0} 项 / 删除 {1} 项 / 回收 {2} MB / 跳过 {3} 项 ====" -f $script:moved, $script:deleted, [math]::Round($script:reclaimedBytes / 1MB, 1), $script:skipped)
