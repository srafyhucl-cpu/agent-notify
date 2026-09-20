#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef VersionInfo
  #error VersionInfo is required
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
#ifndef IngressPath
  #error IngressPath is required
#endif
#ifndef IconPath
  #error IconPath is required
#endif

[Setup]
AppId={{E34E8A9A-4D6B-4C0A-8E2A-8EC8D1A4D8E7}
AppName=AgentNotify Rust Preview
AppVersion={#AppVersion}
AppVerName=AgentNotify Rust Preview {#AppVersion}
AppPublisher=Agent-notify
DefaultDirName={localappdata}\Programs\AgentNotify-Rust-Preview
DefaultGroupName=AgentNotify Rust Preview
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=Agent-notify-Rust-Preview-Setup-v{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
UninstallDisplayName=AgentNotify Rust Preview
UninstallDisplayIcon={app}\agentnotify-desktop.exe
SetupIconFile={#IconPath}
VersionInfoVersion={#VersionInfo}
VersionInfoProductName=AgentNotify Rust Preview
VersionInfoProductVersion={#VersionInfo}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#ifdef SignToolCommand
SignTool=agentnotify
#endif

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agentnotify-desktop.exe"; Flags: ignoreversion
Source: "{#IngressPath}"; DestDir: "{app}"; DestName: "agentnotify-ingress.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\AgentNotify Rust Preview"; Filename: "{app}\agentnotify-desktop.exe"
