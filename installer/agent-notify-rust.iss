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

; AgentNotify Rust 预览安装器。
; 2.0.0 起正式安装入口由 installer\agent-notify.iss 提供，本脚本仅供本地预览与联调使用：
; 使用独立 AppId 与独立安装目录，不会覆盖正式安装。
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

[Tasks]
Name: "opencode"; Description: "接入 OpenCode 通知插件"; GroupDescription: "集成："; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agentnotify-desktop.exe"; Flags: ignoreversion
Source: "{#IngressPath}"; DestDir: "{app}"; DestName: "agentnotify-ingress.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\rust\agent-notify.ts"; DestDir: "{app}\plugin"; DestName: "agent-notify.ts"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-opencode-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion

[Icons]
Name: "{group}\AgentNotify Rust Preview"; Filename: "{app}\agentnotify-desktop.exe"

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Parameters: String;
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('opencode') then
  begin
    Parameters := '-NoProfile -ExecutionPolicy Bypass -File "' +
      ExpandConstant('{app}\tools\hooks\install-opencode-v2.ps1') + '" -Source "' +
      ExpandConstant('{app}\plugin\agent-notify.ts') + '" -Destination "' +
      ExpandConstant('{userprofile}\.config\opencode\plugins\agent-notify.ts') + '" -Ingress "' +
      ExpandConstant('{app}\agentnotify-ingress.exe') + '"';
    if not Exec(
      ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      Parameters,
      '',
      SW_HIDE,
      ewWaitUntilTerminated,
      ResultCode
    ) then
      RaiseException('无法启动 OpenCode 插件安装脚本');
    if ResultCode <> 0 then
      RaiseException('OpenCode 插件安装失败，退出码 ' + IntToStr(ResultCode));
  end;
end;
