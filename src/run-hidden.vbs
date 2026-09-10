' run-hidden.vbs - windowless launcher for linkWeixin ps1 scripts.
'
' Why this exists: on Win11 with Windows Terminal as the default terminal,
' a console window (or WT tab) is created BEFORE powershell.exe gets a chance
' to apply -WindowStyle Hidden. So .lnk shortcuts and scheduled tasks that
' launch powershell.exe directly always flash (and the widget host even keeps
' a permanent empty WT tab). wscript.exe itself has no console, and the child
' powershell starts hidden from birth, so WT never renders anything.
'
' Usage: wscript.exe run-hidden.vbs "<full-path-to-script.ps1>" [args...]
' All comments are ASCII-only on purpose: wscript reads this file in the
' system ANSI codepage, so non-ASCII text would mojibake (comments only,
' harmless, but keep it clean).
Option Explicit
Dim sh, cmd, i
Set sh = CreateObject("Wscript.Shell")
If WScript.Arguments.Count = 0 Then WScript.Quit 1
cmd = "powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File """ & WScript.Arguments(0) & """"
For i = 1 To WScript.Arguments.Count - 1
  cmd = cmd & " """ & WScript.Arguments(i) & """"
Next
sh.Run cmd, 0, False
