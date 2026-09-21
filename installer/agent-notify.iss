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

; AgentNotify 正式安装器（Tauri 桌面版）。
; 保留旧版 AppId 与安装目录，升级会落回原安装位置。
; 卸载只由 Inno 删除本脚本安装的程序文件：不触碰用户数据（配置、凭据、SQLite、迁移报告），
; 也不主动删除用户 OpenCode 配置目录里的插件（插件对 ingress 缺失是容错的，不会打断 OpenCode）。
[Setup]
AppId={{E7A4419F-499D-4A21-BD12-6C2D1F6B31A4}
AppName=AgentNotify
AppVersion={#AppVersion}
AppPublisher=Agent-notify
DefaultDirName={localappdata}\Programs\Agent-notify
DefaultGroupName=AgentNotify
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=Agent-notify-Setup-v{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
UninstallDisplayName=AgentNotify
UninstallDisplayIcon={app}\agentnotify-desktop.exe
SetupIconFile={#RepoRoot}\assets\agent-notify.ico
VersionInfoVersion={#VersionInfo}
VersionInfoProductName=Agent-notify
VersionInfoProductVersion={#VersionInfo}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#ifdef SignToolCommand
SignTool=agentnotify
#endif

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加快捷方式："
Name: "startupicon"; Description: "开机自动启动"; GroupDescription: "启动选项："
Name: "opencode"; Description: "接入 OpenCode 通知插件"; GroupDescription: "集成："

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agentnotify-desktop.exe"; Flags: ignoreversion
Source: "{#IngressPath}"; DestDir: "{app}"; DestName: "agentnotify-ingress.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\rust\agent-notify.ts"; DestDir: "{app}\plugin"; DestName: "agent-notify.ts"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-opencode-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
; 旧版 Hook 清理脚本：升级时用 -HooksOnly 移除指向旧程序的 Codex / Antigravity / Devin Hook 与扩展。
Source: "{#RepoRoot}\uninstall.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hook-config.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "{#RepoRoot}\VERSION"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; 升级清理：旧版是 Go 单文件 + PowerShell 接入脚本，2.0.0 起由桌面端取代。
; 只删程序文件，不触碰用户数据与迁移产物。
Type: files; Name: "{app}\agent-notify.exe"
Type: files; Name: "{app}\agent-notify.manifest"
Type: files; Name: "{app}\install.ps1"
Type: files; Name: "{app}\uninstall.ps1"
Type: files; Name: "{app}\tools\hook-config.ps1"
; 改名前的快捷方式名，安装器升级时一并清掉，避免桌面上出现两个图标或旧程序仍在自启动。
Type: files; Name: "{userdesktop}\Agent-notify.lnk"
Type: files; Name: "{userstartup}\Agent-notify.lnk"
Type: files; Name: "{userdesktop}\Agent-notify 悬浮窗.lnk"
Type: files; Name: "{userstartup}\Agent-notify 悬浮窗.lnk"
; 旧版快捷方式名与新名字相同，先删再建，确保指向新的桌面程序。
Type: files; Name: "{userdesktop}\AgentNotify.lnk"
Type: files; Name: "{userstartup}\AgentNotify.lnk"

[Icons]
Name: "{group}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"
Name: "{userdesktop}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"; Tasks: desktopicon
Name: "{userstartup}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"; Tasks: startupicon

[Run]
Filename: "{app}\agentnotify-desktop.exe"; Description: "启动 AgentNotify"; Flags: nowait postinstall

[Code]
const
  WEBVIEW2_CLIENT_KEY = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function WebView2RuntimeVersion(): String;
var
  Value: String;
begin
  Result := '';
  if RegQueryStringValue(HKLM32, WEBVIEW2_CLIENT_KEY, 'pv', Value) then
    Result := Value
  else if RegQueryStringValue(HKCU, WEBVIEW2_CLIENT_KEY, 'pv', Value) then
    Result := Value;
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  if WebView2RuntimeVersion() = '' then
  begin
    MsgBox('未检测到 Microsoft Edge WebView2 运行时，AgentNotify 的界面无法启动。' + #13#10#13#10 +
      '请先安装 WebView2 Runtime（在微软官网搜索 "WebView2 Runtime" 下载 Evergreen 安装包），' + #13#10 +
      '安装完成后重新运行本安装程序。', mbCriticalError, MB_OK);
    Result := False;
  end;
end;

procedure RunLegacyHookCleanup();
var
  ResultCode: Integer;
  Parameters: String;
begin
  Parameters := '-NoProfile -ExecutionPolicy Bypass -File "' +
    ExpandConstant('{app}\uninstall.ps1') + '" -HooksOnly -InstallDir "' +
    ExpandConstant('{app}') + '"';
  if not Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    Parameters,
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) then
  begin
    MsgBox('无法启动旧版 Hook 清理脚本，Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。', mbError, MB_OK);
    exit;
  end;
  if ResultCode <> 0 then
    MsgBox('旧版 Hook 清理失败（退出码 ' + IntToStr(ResultCode) +
      '），Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。', mbError, MB_OK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Parameters: String;
begin
  if CurStep = ssPostInstall then
  begin
    // 旧 Hook 指向的 agent-notify.exe 已被 [InstallDelete] 移除，先清干净避免用户侧报错。
    // 清理是尽力而为：失败只提示，不阻断安装。
    RunLegacyHookCleanup();
  end;

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
