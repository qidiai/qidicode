# capture_wechat.ps1 — 微信窗口聚焦+定位+截图模板（wechat-file-transfer 技能配套）
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File capture_wechat.ps1
# 输出: %TEMP%\wechat_win.png（返回 RECT 与路径）

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

$src = @'
using System;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class W32 {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
if (-not ([System.Management.Automation.PSTypeName]'W32').Type) {
    Add-Type -TypeDefinition $src -Language CSharp
}

$p = Get-Process Weixin -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
$h = $p.MainWindowHandle
[void][W32]::SetForegroundWindow($h)
Start-Sleep -Milliseconds 500

$r = New-Object RECT
[void][W32]::GetWindowRect($h, [ref]$r)
Write-Output ("RECT L={0} T={1} R={2} B={3}" -f $r.Left, $r.Top, $r.Right, $r.Bottom)

$w = $r.Right - $r.Left
$ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { Write-Output "INVALID RECT"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
$out = Join-Path $env:TEMP "wechat_win.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output ("SAVED {0} ({1} bytes)" -f $out, (Get-Item $out).Length)
Write-Output ("OFFSET wx={0} wy={1} （窗口内相对坐标 (px,py) = 屏幕 (wx+px, wy+py)）" -f $r.Left, $r.Top)
