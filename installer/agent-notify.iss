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
#ifndef CodexHookPath
  #error CodexHookPath is required
#endif
#ifndef AntigravityHookPath
  #error AntigravityHookPath is required
#endif
#ifndef DevinHookPath
  #error DevinHookPath is required
#endif

; AgentNotify 正式安装器（Tauri 桌面版）。
; 保留旧版 AppId 与安装目录，升级会落回原安装位置。
; 卸载只由 Inno 删除本脚本安装的程序文件，再用既有清理脚本移除 AgentNotify 自己写入的
; Hook / 扩展 / mod：不触碰用户数据（配置、凭据、SQLite、迁移报告），
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
; 接入任务默认全部勾选，与 Go 版安装脚本的默认语义一致（不传 -Skip* 就接入全部 Agent）。
Name: "opencode"; Description: "接入 OpenCode 通知插件"; GroupDescription: "集成："
Name: "codex"; Description: "接入 Codex 通知 Hook"; GroupDescription: "集成："
Name: "antigravity"; Description: "接入 Antigravity 通知 Hook"; GroupDescription: "集成："
Name: "devin"; Description: "接入 Devin 通知 Hook 与回复扩展"; GroupDescription: "集成："
Name: "commandcode"; Description: "接入 Command Code 通知 mod"; GroupDescription: "集成："

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agentnotify-desktop.exe"; Flags: ignoreversion
Source: "{#IngressPath}"; DestDir: "{app}"; DestName: "agentnotify-ingress.exe"; Flags: ignoreversion
; 阶段 D 的三个 Hook：与 ingress 同目录，Hook 运行时按"同目录"优先找到 ingress。
Source: "{#CodexHookPath}"; DestDir: "{app}"; DestName: "agentnotify-codex-hook.exe"; Flags: ignoreversion
Source: "{#AntigravityHookPath}"; DestDir: "{app}"; DestName: "agentnotify-antigravity-hook.exe"; Flags: ignoreversion
Source: "{#DevinHookPath}"; DestDir: "{app}"; DestName: "agentnotify-devin-hook.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\rust\agent-notify.ts"; DestDir: "{app}\plugin"; DestName: "agent-notify.ts"; Flags: ignoreversion
; Devin V2 回复扩展与 Command Code V2 mod：先落到安装目录，再由接入脚本按任务部署到用户目录。
Source: "{#RepoRoot}\plugin\devin-extension-v2\package.json"; DestDir: "{app}\plugin\devin-extension-v2"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension-v2\extension.js"; DestDir: "{app}\plugin\devin-extension-v2"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension-v2\acp-bridge.js"; DestDir: "{app}\plugin\devin-extension-v2"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\commandcode-v2\agent-notify.ts"; DestDir: "{app}\plugin\commandcode-v2"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-opencode-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-codex-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-antigravity-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-devin-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hooks\install-commandcode-v2.ps1"; DestDir: "{app}\tools\hooks"; Flags: ignoreversion
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

[UninstallDelete]
; 卸载时清理接入脚本部署到用户目录、Inno 管不到的 AgentNotify 产物。
; 只按精确路径删自己的文件，不递归删除目录，也不触碰用户数据。
; uninstall.ps1 只认 V1 Devin 扩展目录，这里补上 V2 回复扩展的三个文件（删空目录时再删目录本身）。
; 注意：Inno 没有 {userprofile} 常量，用户目录只能用环境变量常量 {%USERPROFILE}。
Type: files; Name: "{%USERPROFILE}\.devin\extensions\agent-notify-reply-v2\package.json"
Type: files; Name: "{%USERPROFILE}\.devin\extensions\agent-notify-reply-v2\extension.js"
Type: files; Name: "{%USERPROFILE}\.devin\extensions\agent-notify-reply-v2\acp-bridge.js"
Type: dirifempty; Name: "{%USERPROFILE}\.devin\extensions\agent-notify-reply-v2"

[Icons]
Name: "{group}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"
Name: "{userdesktop}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"; Tasks: desktopicon
Name: "{userstartup}\AgentNotify"; Filename: "{app}\agentnotify-desktop.exe"; Tasks: startupicon

[Run]
Filename: "{app}\agentnotify-desktop.exe"; Description: "启动 AgentNotify"; Flags: nowait postinstall

[UninstallRun]
; 卸载时先跑既有清理脚本：只移除 AgentNotify 自己写入的 Codex notify、Antigravity / Devin Hook、
; V1 扩展与 Command Code mod（均带归属校验），不删除程序文件、用户数据与 OpenCode 插件。
; 该节在卸载删除文件之前执行，脚本此时仍在 {app} 下。
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"" -HooksOnly"; Flags: runhidden; RunOnceId: "AgentNotifyHooksCleanup"

[Code]
const
  WEBVIEW2_CLIENT_KEY = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

var
  // 本次安装未能完成的 Agent 接入（每行一条）；为空表示全部成功。
  IntegrationFailures: String;

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
    // 静默安装（应用内一键升级）没有可用的提示窗口，也不该悄悄跳过这项检查：
    // 显式 Abort 中止安装，并把原因写进调用方 /LOG= 指定的安装日志。
    // 注意：Inno 的 SuppressibleMsgBox 只在静默安装配合 /SUPPRESSMSGBOXES 时才会被抑制，
    // 所以这里不能只靠换函数名，必须按 WizardSilent 显式分支。
    if WizardSilent then
    begin
      Log('未检测到 Microsoft Edge WebView2 运行时，静默安装中止：桌面端界面无法启动。');
      Abort;
    end;
    SuppressibleMsgBox('未检测到 Microsoft Edge WebView2 运行时，AgentNotify 的界面无法启动。' + #13#10#13#10 +
      '请先安装 WebView2 Runtime（在微软官网搜索 "WebView2 Runtime" 下载 Evergreen 安装包），' + #13#10 +
      '安装完成后重新运行本安装程序。', mbCriticalError, MB_OK, IDOK);
    Result := False;
  end;
end;

// 静默安装（应用内一键升级）在替换文件前强制结束会占用待替换文件的旧进程，让 Restart Manager 无物可关。
// 起因：Go 版悬浮窗收到关闭请求会隐藏到托盘而不退出，Restart Manager 关不掉它，安装器就会停在
// 「无法自动关闭所有应用程序」；该提示只在 /SILENT 配合 /SUPPRESSMSGBOXES 时才被抑制，
// 而 /SUPPRESSMSGBOXES 会把这种情况变成静默中止安装，所以只能消除原因，不能靠压制提示。
// 只在静默下强杀：用户点「升级」即授权这次静默替换；交互式安装保持原行为，仍由用户在提示里自己决定。
// 刻意不抽公共子过程：强杀逻辑集中在这一个带静默守卫的过程里，便于人工审计与结构测试断言。
procedure TerminateStaleInstancesForSilentUpgrade();
var
  ResultCode: Integer;
begin
  if not WizardSilent then
    exit;
  // 进程不存在时 taskkill 返回非 0，属正常情况：只写日志、不算失败。
  if Exec(
    ExpandConstant('{sys}\taskkill.exe'),
    '/F /IM agent-notify.exe',
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) then
    Log('静默升级：已调用 taskkill 结束旧版 agent-notify.exe，退出码 ' + IntToStr(ResultCode) +
      '（进程不存在或结束失败都不影响后续安装）。')
  else
    Log('静默升级：无法启动 taskkill 结束旧版 agent-notify.exe，交由 Restart Manager 处理。');
  if Exec(
    ExpandConstant('{sys}\taskkill.exe'),
    '/F /IM agentnotify-desktop.exe',
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) then
    Log('静默升级：已调用 taskkill 结束 agentnotify-desktop.exe，退出码 ' + IntToStr(ResultCode) +
      '（进程不存在或结束失败都不影响后续安装）。')
  else
    Log('静默升级：无法启动 taskkill 结束 agentnotify-desktop.exe，交由 Restart Manager 处理。');
end;

// 升级后界面停在旧版是 WebView2 磁盘缓存造成的：缓存里的 index.html 仍指向上一版的前端包。
// 2026-09-26 在 2.0.7 真实发生过（新程序配上 2.0.6 时代的 Code Cache，界面停在旧版）。
// 只清三类缓存目录，Local Storage 一律保留——自定义账号名、主题选择都存在那里。
// 失败只记日志不打断安装：缓存缺失最多让首屏稍慢，不该阻止升级。
procedure ClearWebViewCache();
var
  CacheRoot: String;
  Names: TArrayOfString;
  Index: Integer;
begin
  CacheRoot := ExpandConstant('{localappdata}\com.agentnotify.desktop\EBWebView\Default');
  SetArrayLength(Names, 3);
  Names[0] := 'Cache';
  Names[1] := 'Code Cache';
  Names[2] := 'GPUCache';
  for Index := 0 to GetArrayLength(Names) - 1 do
  begin
    if DirExists(CacheRoot + '\' + Names[Index]) then
    begin
      if DelTree(CacheRoot + '\' + Names[Index], True, True, True) then
        Log('已清理 WebView2 缓存：' + Names[Index])
      else
        Log('未能完整清理 WebView2 缓存（可忽略，界面可能短暂停留在旧版）：' + Names[Index]);
    end;
  end;
end;

// PrepareToInstall 在 Setup 检查文件占用（CloseApplications 的 Restart Manager 阶段）之前调用，
// 是官方文档指定用于关闭待更新应用的时机；错过它就会在替换文件前弹出「无法自动关闭所有应用程序」。
// NeedsRestart 有意不动：本过程没有重启需求，也不替 Setup 决定是否提示重启。
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  TerminateStaleInstancesForSilentUpgrade();
  ClearWebViewCache();
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
    // 清理是尽力而为的一步，成败语义不变：静默安装只写日志（没有可见提示窗口），照样继续安装。
    if WizardSilent then
      Log('无法启动旧版 Hook 清理脚本，已跳过清理：Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。')
    else
      SuppressibleMsgBox('无法启动旧版 Hook 清理脚本，Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。', mbError, MB_OK, IDOK);
    exit;
  end;
  if ResultCode <> 0 then
  begin
    if WizardSilent then
      Log('旧版 Hook 清理失败（退出码 ' + IntToStr(ResultCode) +
        '）：Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。')
    else
      SuppressibleMsgBox('旧版 Hook 清理失败（退出码 ' + IntToStr(ResultCode) +
        '），Codex / Antigravity / Devin 的旧 Hook 可能仍指向已移除的程序。', mbError, MB_OK, IDOK);
  end;
end;

// 记录一条接入失败；接入失败不影响程序本体，安装结束后统一提示（静默安装只写日志）。
procedure AddIntegrationFailure(const Message: String);
begin
  if IntegrationFailures <> '' then
    IntegrationFailures := IntegrationFailures + #13#10;
  IntegrationFailures := IntegrationFailures + Message;
end;

// 按任务执行一个接入脚本。失败只记录、不抛异常：与旧版 Hook 清理同样是尽力而为，
// 避免用户自己的配置（多行 notify、JSON 格式错误）把整个安装判为失败；
// 失败原因由安装结束时的提示给出，桌面端 Agents 页也能看到接入状态。
procedure RunIntegration(const DisplayName: String; const ScriptName: String; const Parameters: String);
var
  ResultCode: Integer;
begin
  if not Exec(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -ExecutionPolicy Bypass -File "' + ExpandConstant('{app}\tools\hooks\' + ScriptName) + '" ' + Parameters,
    '',
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode
  ) then
  begin
    AddIntegrationFailure(DisplayName + '：无法启动 PowerShell 运行 ' + ScriptName);
    exit;
  end;
  if ResultCode <> 0 then
    AddIntegrationFailure(DisplayName + '：' + ScriptName + ' 退出码 ' + IntToStr(ResultCode));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  Parameters: String;
begin
  if CurStep <> ssPostInstall then
    exit;

  // 旧 Hook 指向的 agent-notify.exe 已被 [InstallDelete] 移除，先清干净避免用户侧报错。
  // 清理是尽力而为：失败只提示，不阻断安装。它必须先于下面的新 Hook 接入执行，
  // 否则会把本次刚写入的 Hook 配置当成旧 Hook 清掉。
  RunLegacyHookCleanup();

  if WizardIsTaskSelected('opencode') then
  begin
    // 用户目录只能用环境变量常量 {%USERPROFILE}：Inno 没有 {userprofile} 常量，
    // 写错会在安装末尾抛异常（ExpandConstant 对未知常量直接报错）。
    Parameters := '-NoProfile -ExecutionPolicy Bypass -File "' +
      ExpandConstant('{app}\tools\hooks\install-opencode-v2.ps1') + '" -Source "' +
      ExpandConstant('{app}\plugin\agent-notify.ts') + '" -Destination "' +
      ExpandConstant('{%USERPROFILE}\.config\opencode\plugins\agent-notify.ts') + '" -Ingress "' +
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

  // 阶段 D 的四个 Agent 接入：Hook 与 ingress 都用本次安装目录的绝对路径，
  // 不依赖接入脚本默认的 %USERPROFILE%\bin；其余路径（Codex 配置、Devin 扩展目录等）
  // 由脚本按与桌面端一致的默认位置解析。
  if WizardIsTaskSelected('codex') then
    RunIntegration('Codex', 'install-codex-v2.ps1',
      '-HookPath "' + ExpandConstant('{app}\agentnotify-codex-hook.exe') + '"' +
      ' -Ingress "' + ExpandConstant('{app}\agentnotify-ingress.exe') + '"');

  if WizardIsTaskSelected('antigravity') then
    RunIntegration('Antigravity', 'install-antigravity-v2.ps1',
      '-HookPath "' + ExpandConstant('{app}\agentnotify-antigravity-hook.exe') + '"' +
      ' -Ingress "' + ExpandConstant('{app}\agentnotify-ingress.exe') + '"');

  if WizardIsTaskSelected('devin') then
    RunIntegration('Devin', 'install-devin-v2.ps1',
      '-HookPath "' + ExpandConstant('{app}\agentnotify-devin-hook.exe') + '"' +
      ' -ExtensionSource "' + ExpandConstant('{app}\plugin\devin-extension-v2') + '"' +
      ' -Ingress "' + ExpandConstant('{app}\agentnotify-ingress.exe') + '"');

  if WizardIsTaskSelected('commandcode') then
    RunIntegration('Command Code', 'install-commandcode-v2.ps1',
      '-Source "' + ExpandConstant('{app}\plugin\commandcode-v2\agent-notify.ts') + '"' +
      ' -Ingress "' + ExpandConstant('{app}\agentnotify-ingress.exe') + '"');

  if IntegrationFailures <> '' then
  begin
    // 接入失败不影响程序本体，成败语义不变：静默安装只写日志，安装照样算成功
    // （桌面端 Agents 页能看到每个 Agent 的接入状态）。
    if WizardSilent then
      Log('以下 Agent 接入没有完成（静默安装只记录日志），其它安装内容不受影响：' + #13#10 + IntegrationFailures)
    else
      SuppressibleMsgBox('以下 Agent 接入没有完成，其它安装内容不受影响：' + #13#10#13#10 +
        IntegrationFailures + #13#10#13#10 +
        '可在桌面端 Agents 页查看接入状态；需要重试时手动运行' + #13#10 +
        ExpandConstant('{app}\tools\hooks') + #13#10 +
        '下对应 Agent 的 install-*.ps1（用 -HookPath / -Ingress 指向本安装目录）。', mbError, MB_OK, IDOK);
  end;
end;
