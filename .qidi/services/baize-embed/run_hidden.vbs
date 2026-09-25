' Launch server.py (baize-embed) with no console window. wscript is a GUI host.
' Spawned by restart_baize.ps1 via WMI so the python child survives callers.
' All ASCII on purpose; paths resolved from this script's own folder.
Set fso = CreateObject("Scripting.FileSystemObject")
Set sh  = CreateObject("WScript.Shell")
here = fso.GetParentFolderName(WScript.ScriptFullName)
sh.CurrentDirectory = here
sh.Run """C:\Program Files\Python312\python.exe"" """ & here & """\server.py""", 0, False