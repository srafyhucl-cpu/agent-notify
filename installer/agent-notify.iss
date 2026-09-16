#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef RepoRoot
  #error RepoRoot is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef ExePath
  #error ExePath is required
#endif

[Setup]
AppId={{E7A4419F-499D-4A21-BD12-6C2D1F6B31A4}
AppName=Agent-notify
AppVersion={#AppVersion}
AppPublisher=Agent-notify
DefaultDirName={localappdata}\Programs\Agent-notify
DefaultGroupName=Agent-notify
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=Agent-notify-Setup-v{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayName=Agent-notify
UninstallDisplayIcon={app}\agent-notify.exe
VersionInfoVersion={#AppVersion}
VersionInfoProductName=Agent-notify
VersionInfoProductVersion={#AppVersion}
#ifdef SignToolCommand
SignTool=agentnotify
#endif

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加快捷方式："
Name: "startupicon"; Description: "开机自动启动悬浮窗"; GroupDescription: "启动选项："

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agent-notify.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\VERSION"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\agent-notify.ts"; DestDir: "{app}\plugin"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\package.json"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\extension.js"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\acp-bridge.js"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\install.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\uninstall.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hook-config.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion

[InstallDelete]
; 旧版本由 install.ps1 创建过另一个名字的快捷方式，安装器升级时一并清掉，避免重复。
Type: files; Name: "{userdesktop}\Agent-notify 悬浮窗.lnk"
Type: files; Name: "{userstartup}\Agent-notify 悬浮窗.lnk"

[Icons]
Name: "{group}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"
Name: "{userdesktop}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Tasks: desktopicon
Name: "{userstartup}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Tasks: startupicon

[Run]
Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Description: "启动 Agent-notify"; Flags: nowait postinstall

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"" -InstallDir ""{app}"" -SkipProcessStop -SkipShortcuts"; Flags: runhidden; RunOnceId: "AgentNotifyCleanup"
