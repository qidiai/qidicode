# ============================================================
#  pick_folder.ps1 - Pop a folder picker, output the chosen path.
#  Extracted from "在指定目录启动qidi.bat" to avoid multi-level
#  quote nesting in the bat's `for /f` inline powershell call.
# ============================================================
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.FolderBrowserDialog
$f.Description = 'Select the project folder to start QIDI Code in'
if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    $f.SelectedPath
}