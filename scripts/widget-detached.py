"""Detached launcher for the linkWeixin widget (Windows only).

Why this exists: every console-subsystem host (powershell.exe), even one
started hidden, gets a Windows Terminal tab on Win11. Closing that tab
kills the attached powershell (and the widget) instantly with zero logs,
and there is no way to tell the two apart. pythonw.exe is GUI-subsystem
(no console ever) and it spawns powershell with CREATE_NO_WINDOW, so the
widget ends up with no console, no WT tab, nothing to close by accident.

Usage: pythonw.exe widget-detached.py
(It must sit next to linkweixin-widget.ps1; both live in ~/bin.)
"""
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PS1 = os.path.join(HERE, "linkweixin-widget.ps1")

CREATE_NO_WINDOW = 0x08000000

if not os.path.isfile(PS1):
    sys.exit(1)
subprocess.Popen(
    ["powershell.exe", "-NoProfile", "-WindowStyle", "Hidden",
     "-ExecutionPolicy", "Bypass", "-File", PS1],
    creationflags=CREATE_NO_WINDOW,
    start_new_session=True,
    stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
    close_fds=True,
)
