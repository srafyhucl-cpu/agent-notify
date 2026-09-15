# Agent-notify Windows 标准安装器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Agent-notify 增加当前用户级 Windows 安装器、首次启动静默接入和基于安装器的应用内升级，同时保留现有 ZIP 与旧版更新兼容。

**Architecture:** Inno Setup 生成 `Agent-notify-Setup-vX.Y.Z.exe`，安装程序文件并创建标准快捷方式与卸载入口。首次启动调用 `install.ps1 -ConfigureOnly` 的隐藏子进程，安装或更新用户级插件和 Hook。更新模块优先下载安装器并校验 SHA256，旧 Release 缺少安装器时继续使用 ZIP。

**Tech Stack:** Go 1.25、Windows PowerShell 5.1、Inno Setup 6、PowerShell、GitHub Actions

## Global Constraints

- 普通用户入口必须是 `Agent-notify-Setup-vX.Y.Z.exe`，不得要求用户运行 PowerShell。
- 默认按当前用户安装到 `%LOCALAPPDATA%\Programs\Agent-notify`，不要求管理员权限。
- Release 必须同时保留 Setup EXE、ZIP 和包含两者 SHA256 的 `SHA256SUMS.txt`。
- 版本号唯一来源是 `internal/app/version.go`。
- 保留旧版客户端通过 ZIP 更新到新版的兼容路径。
- 不删除 ClawBot 凭据、配置、推送历史、引用路由和用户自定义 Hook。
- 不在 C 盘创建项目下载、安装器构建缓存或临时目录；本地工具使用 `D:\Temp`。
- 所有新增注释和用户可见错误使用中文。

---

### Task 1: 增加安装脚本的仅配置模式

**Files:**
- Modify: `install.ps1:20-42`
- Modify: `install.ps1:210-430`
- Test: `tests/smoke.ps1`

**Interfaces:**
- Consumes: 已安装目录中的 `agent-notify.exe`、`plugin/agent-notify.ts`、`plugin/devin-extension/*`
- Produces: `install.ps1 -ConfigureOnly -InstallDir <path> -SkipWidgetLaunch -SkipLoginLaunch -SkipShortcuts`
- Produces: 配置文件成功时退出码为 `0`；任何原子写入或 Hook 冲突失败时退出码非 `0`

- [ ] **Step 1: 在冒烟测试中增加失败用例**

在 `tests/smoke.ps1` 的安装器测试区域加入：

```powershell
$configureOnlyRoot = Join-Path $Root 'tmp-configure-only'
$configureOnlyInstall = Join-Path $configureOnlyRoot 'app'
$configureOnlyPlugin = Join-Path $configureOnlyRoot 'plugins'
New-Item -ItemType Directory -Force -Path (Join-Path $configureOnlyInstall 'plugin\devin-extension') | Out-Null
Copy-Item -LiteralPath (Join-Path $Root 'bin\agent-notify.exe') -Destination (Join-Path $configureOnlyInstall 'agent-notify.exe')
Copy-Item -LiteralPath (Join-Path $Root 'plugin\agent-notify.ts') -Destination (Join-Path $configureOnlyInstall 'plugin\agent-notify.ts')
Copy-Item -LiteralPath (Join-Path $Root 'plugin\devin-extension\package.json') -Destination (Join-Path $configureOnlyInstall 'plugin\devin-extension\package.json')
Copy-Item -LiteralPath (Join-Path $Root 'plugin\devin-extension\extension.js') -Destination (Join-Path $configureOnlyInstall 'plugin\devin-extension\extension.js')
Copy-Item -LiteralPath (Join-Path $Root 'plugin\devin-extension\acp-bridge.js') -Destination (Join-Path $configureOnlyInstall 'plugin\devin-extension\acp-bridge.js')

$configureOutput = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Root 'install.ps1') `
  -ConfigureOnly `
  -InstallDir $configureOnlyInstall `
  -PluginDir $configureOnlyPlugin `
  -DevinExtensionDir (Join-Path $configureOnlyRoot 'devin-extension') `
  -CodexConfig (Join-Path $configureOnlyRoot 'codex\config.toml') `
  -AntigravityHooks (Join-Path $configureOnlyRoot 'gemini\hooks.json') `
  -DevinConfig (Join-Path $configureOnlyRoot 'devin\config.json') `
  -SkipShortcuts `
  -SkipWidgetLaunch `
  -SkipLoginLaunch 2>&1
if ($LASTEXITCODE -ne 0) {
  throw "ConfigureOnly 安装失败：$($configureOutput -join [Environment]::NewLine)"
}
if (-not (Test-Path -LiteralPath (Join-Path $configureOnlyPlugin 'agent-notify.ts'))) {
  throw 'ConfigureOnly 未安装 OpenCode 插件'
}
if (-not (Test-Path -LiteralPath (Join-Path $configureOnlyRoot 'devin-extension\package.json'))) {
  throw 'ConfigureOnly 未安装 Devin 扩展'
}
```

- [ ] **Step 2: 运行冒烟测试并确认失败**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\smoke.ps1
```

Expected: FAIL，提示 `ConfigureOnly 安装失败` 或参数无法识别。

- [ ] **Step 3: 增加参数并让包检测识别安装目录结构**

在 `install.ps1` 参数块加入：

```powershell
  [switch]$ConfigureOnly,
```

将 `$HasPackage` 改为：

```powershell
$HasPackage = (
  (Test-Path (Join-Path $RepoRoot "bin\$ExeName")) -or
  ($ConfigureOnly -and (Test-Path (Join-Path $RepoRoot $ExeName)))
) -and
  (Test-Path (Join-Path $RepoRoot "plugin\$PluginName")) -and
  $HasDevinExtension
```

在源码/发布包自检中，仅配置模式要求：

```powershell
if ($ConfigureOnly) {
  foreach ($required in @(
      $ExeName,
      'plugin\agent-notify.ts',
      'plugin\devin-extension\package.json',
      'plugin\devin-extension\extension.js',
      'plugin\devin-extension\acp-bridge.js'
    )) {
    if (-not (Test-Path (Join-Path $RepoRoot $required))) {
      throw "仅配置模式的安装目录缺文件：$required"
    }
  }
} elseif ($HasSource) {
  # 保留现有源码自检
}
```

- [ ] **Step 4: 跳过 exe 自替换并复用现有插件、扩展和 Hook 配置路径**

在进入现有构建逻辑前设置：

```powershell
$installedExe = Join-Path $InstallDir $ExeName
if ($ConfigureOnly) {
  if (-not (Test-Path -LiteralPath $installedExe -PathType Leaf)) {
    throw "仅配置模式找不到已安装程序：$installedExe"
  }
} else {
  # 保留现有 $repoExe、编译、停止进程和 Install-FileAtomically 逻辑
}
```

仅配置模式不得执行以下操作：

```powershell
# 以下操作只在 -not $ConfigureOnly 时执行
Initialize-GoEnvironment
Install-FileAtomically -Source $repoExe -Destination $installedExe
Start-Process $installedExe -ArgumentList @('login')
```

把快捷方式条件改为：

```powershell
if (-not $SkipShortcuts -and -not $ConfigureOnly) {
```

把悬浮窗启动条件改为：

```powershell
if (-not $SkipWidgetLaunch -and -not $ConfigureOnly) {
```

保留现有以下行为，因为它们是首次启动要完成的实际工作：

- 复制并修补 OpenCode 插件中的 `BAKED_BIN`
- 复制 Devin 回复扩展
- 写入 Antigravity 和 Devin Hook
- 在安全条件下接管 Codex notify
- 写入 `agent-notify-install.json`

- [ ] **Step 5: 运行仅配置模式测试**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\smoke.ps1
```

Expected: PASS，且仅配置模式不会尝试编译或替换 `agent-notify.exe`。

- [ ] **Step 6: 提交**

```powershell
git add install.ps1 tests/smoke.ps1
git commit -m "feat(install): 增加仅配置模式"
```

---

### Task 2: 增加首次启动静默初始化

**Files:**
- Create: `internal/setup/setup.go`
- Create: `internal/setup/setup_windows.go`
- Create: `internal/setup/setup_other.go`
- Create: `internal/setup/setup_test.go`
- Modify: `cmd/agent-notify/main.go:139-219`
- Modify: `internal/config/paths.go:6-120`
- Modify: `internal/config/config_test.go:113-180`

**Interfaces:**

```go
type Options struct {
	Version     string
	InstallDir  string
	ScriptPath  string
	StateFile   string
	LogFile     string
}

func Ensure(ctx context.Context, options Options) error
func IsComplete(stateFile, version string) bool
```

- [ ] **Step 1: 写状态文件与参数测试**

创建 `internal/setup/setup_test.go`：

```go
//go:build windows

package setup

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func TestEnsureSkipsCompletedVersion(t *testing.T) {
	root := t.TempDir()
	state := filepath.Join(root, "setup-state.json")
	if err := writeState(state, "1.4.2"); err != nil {
		t.Fatal(err)
	}
	called := false
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: filepath.Join(root, "install.ps1"),
		StateFile:  state,
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(context.Context, string, ...string) ([]byte, error) {
		called = true
		return nil, nil
	})
	if err != nil || called {
		t.Fatalf("Ensure completed err=%v called=%v", err, called)
	}
}

func TestEnsureBuildsConfigureOnlyCommand(t *testing.T) {
	root := t.TempDir()
	script := filepath.Join(root, "install.ps1")
	if err := os.WriteFile(script, []byte("param()"), 0600); err != nil {
		t.Fatal(err)
	}
	var gotName string
	var gotArgs []string
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: script,
		StateFile:  filepath.Join(root, "setup-state.json"),
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(_ context.Context, name string, args ...string) ([]byte, error) {
		gotName = name
		gotArgs = append([]string(nil), args...)
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	want := []string{
		"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script,
		"-ConfigureOnly", "-InstallDir", root,
		"-SkipWidgetLaunch", "-SkipLoginLaunch", "-SkipShortcuts",
	}
	if gotName != "powershell.exe" || !reflect.DeepEqual(gotArgs, want) {
		t.Fatalf("command = %s %#v", gotName, gotArgs)
	}
}

func TestEnsureDoesNotMarkFailureComplete(t *testing.T) {
	root := t.TempDir()
	script := filepath.Join(root, "install.ps1")
	if err := os.WriteFile(script, []byte("param()"), 0600); err != nil {
		t.Fatal(err)
	}
	state := filepath.Join(root, "setup-state.json")
	err := ensure(context.Background(), Options{
		Version:    "1.4.2",
		InstallDir: root,
		ScriptPath: script,
		StateFile:  state,
		LogFile:    filepath.Join(root, "setup.log"),
	}, func(context.Context, string, ...string) ([]byte, error) {
		return []byte("hook conflict"), errors.New("exit status 1")
	})
	if err == nil || IsComplete(state, "1.4.2") {
		t.Fatalf("Ensure failure err=%v complete=%v", err, IsComplete(state, "1.4.2"))
	}
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/setup -run TestEnsure -v
```

Expected: FAIL，`internal/setup` 包不存在。

- [ ] **Step 3: 实现状态文件和可注入命令执行器**

创建 `internal/setup/setup.go`：

```go
package setup

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type Options struct {
	Version    string
	InstallDir string
	ScriptPath string
	StateFile  string
	LogFile    string
}

type commandRunner func(context.Context, string, ...string) ([]byte, error)

type state struct {
	Version     string `json:"version"`
	CompletedAt string `json:"completedAt"`
}

func Ensure(ctx context.Context, options Options) error {
	return ensure(ctx, options, runHidden)
}

func ensure(ctx context.Context, options Options, runner commandRunner) error {
	if IsComplete(options.StateFile, options.Version) {
		return nil
	}
	if strings.TrimSpace(options.InstallDir) == "" {
		return errors.New("安装目录为空")
	}
	if _, err := os.Stat(options.ScriptPath); err != nil {
		return fmt.Errorf("首次配置程序不存在：%w", err)
	}
	args := []string{
		"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", options.ScriptPath,
		"-ConfigureOnly", "-InstallDir", options.InstallDir,
		"-SkipWidgetLaunch", "-SkipLoginLaunch", "-SkipShortcuts",
	}
	output, err := runner(ctx, "powershell.exe", args...)
	if err != nil {
		_ = os.MkdirAll(filepath.Dir(options.LogFile), 0700)
		_ = os.WriteFile(options.LogFile, output, 0600)
		return fmt.Errorf("首次接入失败：%w", err)
	}
	return writeState(options.StateFile, options.Version)
}

func IsComplete(stateFile, version string) bool {
	data, err := os.ReadFile(stateFile)
	if err != nil {
		return false
	}
	var value state
	return json.Unmarshal(data, &value) == nil && value.Version == version
}

func writeState(path, version string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(state{
		Version:     version,
		CompletedAt: time.Now().Format(time.RFC3339),
	}, "", "  ")
	if err != nil {
		return err
	}
	temporary := path + ".new"
	if err := os.WriteFile(temporary, append(data, '\n'), 0600); err != nil {
		return err
	}
	return os.Rename(temporary, path)
}
```

创建 `internal/setup/setup_windows.go`：

```go
//go:build windows

package setup

import (
	"context"
	"os/exec"
	"syscall"
)

const createNoWindow = 0x08000000

func runHidden(ctx context.Context, name string, args ...string) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	command.SysProcAttr = &syscall.SysProcAttr{
		HideWindow:    true,
		CreationFlags: createNoWindow,
	}
	return command.CombinedOutput()
}
```

创建 `internal/setup/setup_other.go`，保证非 Windows 环境可以完成包编译：

```go
//go:build !windows

package setup

import (
	"context"
	"errors"
)

func runHidden(context.Context, string, ...string) ([]byte, error) {
	return nil, errors.New("首次接入仅支持 Windows")
}
```

- [ ] **Step 4: 增加状态文件路径**

在 `internal/config/paths.go` 的 `Paths` 增加：

```go
	SetupStateFile          string
	SetupLogFile            string
```

在 `GetPaths` 中加入：

```go
	setupStateFile := envOr("AGENT_NOTIFY_SETUP_STATE_FILE", filepath.Join(configDir, "setup-state.json"))
	setupLogFile := envOr("AGENT_NOTIFY_SETUP_LOG_FILE", filepath.Join(tempDir, "setup.log"))
```

在返回的结构体中加入：

```go
		SetupStateFile:          setupStateFile,
		SetupLogFile:            setupLogFile,
```

在 `internal/config/config_test.go` 的覆盖测试中断言：

```go
if paths.SetupStateFile != filepath.Join(dir, "setup-state.json") {
	t.Fatalf("SetupStateFile = %q", paths.SetupStateFile)
}
```

- [ ] **Step 5: 把初始化接入无参数启动和 widget 启动**

在 `cmd/agent-notify/main.go` 增加：

```go
func runWidget() {
	executable, _ := os.Executable()
	installDir := filepath.Dir(executable)
	paths := config.GetPaths()
	setupError := setup.Ensure(context.Background(), setup.Options{
		Version:    app.Version,
		InstallDir: installDir,
		ScriptPath: filepath.Join(installDir, "install.ps1"),
		StateFile:  paths.SetupStateFile,
		LogFile:    paths.SetupLogFile,
	})
	ui.RunWidget(ui.WidgetOptions{
		InitialSetupError: setupError,
		RepairSetup: func(ctx context.Context) error {
			return setup.Ensure(ctx, setup.Options{
				Version:    app.Version,
				InstallDir: installDir,
				ScriptPath: filepath.Join(installDir, "install.ps1"),
				StateFile:  paths.SetupStateFile,
				LogFile:    paths.SetupLogFile,
			})
		},
	})
}
```

把无参数分支和 `widget` 分支都改为调用 `runWidget()`。仅为开发版本和无安装脚本的源码运行增加显式跳过：

```go
if app.Version == "dev" {
	if _, err := os.Stat(filepath.Join(installDir, "install.ps1")); os.IsNotExist(err) {
		setupError = nil
	}
}
```

- [ ] **Step 6: 运行初始化测试**

Run:

```powershell
go test ./internal/setup ./internal/config ./cmd/agent-notify -v
```

Expected: PASS；完成版本不重复执行，失败不写完成状态，命令参数无 shell 拼接。

- [ ] **Step 7: 提交**

```powershell
git add internal/setup internal/config/paths.go internal/config/config_test.go cmd/agent-notify/main.go
git commit -m "feat(setup): 首次启动静默完成接入"
```

---

### Task 3: 在悬浮窗展示初始化失败并支持重试

**Files:**
- Modify: `internal/ui/widget.go:302-780`
- Test: `internal/ui/agent_cards_test.go`

**Interfaces:**

```go
type WidgetOptions struct {
	InitialSetupError error
	RepairSetup       func(context.Context) error
}

func RunWidget(options WidgetOptions)
```

- [ ] **Step 1: 写失败状态测试**

在 `internal/ui/agent_cards_test.go` 增加：

```go
func TestHealthReportsSetupFailure(t *testing.T) {
	app := WidgetApp{
		clawbotLoggedIn:     true,
		clawbotSessionReady: true,
		setupError:          "首次接入失败：hook 被占用",
	}
	_, label := app.health()
	if label != "接入异常" {
		t.Fatalf("health label = %q", label)
	}
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/ui -run TestHealthReportsSetupFailure -v
```

Expected: FAIL，`WidgetApp` 没有 `setupError`。

- [ ] **Step 3: 增加 WidgetOptions 和错误状态**

在 `internal/ui/widget.go` 增加：

```go
type WidgetOptions struct {
	InitialSetupError error
	RepairSetup       func(context.Context) error
}
```

在 `WidgetApp` 增加：

```go
	setupError string
	repairSetup func(context.Context) error
```

把 `RunWidget()` 改为：

```go
func RunWidget(options WidgetOptions) {
	// 保留现有窗口创建流程；创建 instance 时写入：
	// instance.setupError = errorText(options.InitialSetupError)
	// instance.repairSetup = options.RepairSetup
}
```

在 `health()` 最前面加入：

```go
if app.setupError != "" {
	return RGB(224, 165, 70), "接入异常"
}
```

- [ ] **Step 4: 让“检查修复”重跑首次配置**

在 `repairIntegrations` 的现有 Codex 修复前执行：

```go
if app.repairSetup != nil {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	err := app.repairSetup(ctx)
	cancel()
	if err != nil {
		failures = append(failures, "首次接入："+err.Error())
	} else {
		app.setupError = ""
	}
}
```

错误弹窗必须保留现有各 Agent 状态，并优先显示 `failures`。

- [ ] **Step 5: 运行 UI 测试**

Run:

```powershell
go test ./internal/ui -v
```

Expected: PASS。

- [ ] **Step 6: 提交**

```powershell
git add internal/ui/widget.go internal/ui/agent_cards_test.go
git commit -m "feat(ui): 展示首次接入失败并支持重试"
```

---

### Task 4: 增加 Inno Setup 安装器与构建入口

**Files:**
- Create: `installer/agent-notify.iss`
- Create: `tools/build-installer.ps1`
- Modify: `tools/build-release.ps1:1-180`
- Modify: `.github/workflows/release.yml:1-90`
- Test: `tests/installer-smoke.ps1`

**Interfaces:**

- Produces: `dist/Agent-notify-Setup-v<version>.exe`
- Produces: `dist/SHA256SUMS.txt`，同时包含 Setup EXE 和 ZIP
- Consumes: `AGENT_NOTIFY_ISCC` 可覆盖 ISCC 路径

- [ ] **Step 1: 创建安装器冒烟测试**

创建 `tests/installer-smoke.ps1`：

```powershell
#Requires -Version 5.1
param(
  [Parameter(Mandatory = $true)][string]$Installer
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
  throw "安装器不存在：$Installer"
}

$stream = [IO.File]::OpenRead($Installer)
try {
  $reader = New-Object IO.BinaryReader($stream)
  if ($reader.ReadByte() -ne 0x4d -or $reader.ReadByte() -ne 0x5a) {
    throw '安装器不是 Windows PE 文件'
  }
} finally {
  $stream.Dispose()
}

$versionInfo = (Get-Item -LiteralPath $Installer).VersionInfo
if ([string]::IsNullOrWhiteSpace($versionInfo.ProductName)) {
  throw '安装器缺少产品版本信息'
}
Write-Output '[installer-smoke] 安装器结构检查通过'
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -Installer .\dist\missing.exe
```

Expected: FAIL，`安装器不存在`。

- [ ] **Step 3: 创建 Inno Setup 脚本**

创建 `installer/agent-notify.iss`：

```iss
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

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加快捷方式："
Name: "startupicon"; Description: "开机自动启动悬浮窗"; GroupDescription: "启动选项："

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "agent-notify.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\agent-notify.ts"; DestDir: "{app}\plugin"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\package.json"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\extension.js"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\plugin\devin-extension\acp-bridge.js"; DestDir: "{app}\plugin\devin-extension"; Flags: ignoreversion
Source: "{#RepoRoot}\install.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\uninstall.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\tools\hook-config.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion

[Icons]
Name: "{group}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"
Name: "{userdesktop}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Tasks: desktopicon
Name: "{userstartup}\Agent-notify"; Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Tasks: startupicon

[Run]
Filename: "{app}\agent-notify.exe"; Parameters: "widget"; Description: "启动 Agent-notify"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\uninstall.ps1"" -InstallDir ""{app}"" -SkipProcessStop -SkipShortcuts"; Flags: runhidden; RunOnceId: "AgentNotifyCleanup"
```

- [ ] **Step 4: 创建安装器构建脚本**

创建 `tools/build-installer.ps1`：

```powershell
#Requires -Version 5.1
param(
  [string]$Version,
  [string]$OutDir,
  [string]$ExePath
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }
if (-not $ExePath) { $ExePath = Join-Path $RepoRoot 'bin\agent-notify.exe' }
if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
  throw "找不到待打包的 agent-notify.exe：$ExePath"
}
if (-not $Version) {
  $match = Select-String -Path (Join-Path $RepoRoot 'internal\app\version.go') -Pattern 'Version\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $match) { throw '无法读取应用版本' }
  $Version = $match.Matches[0].Groups[1].Value
}

function Resolve-Iscc {
  if ($env:AGENT_NOTIFY_ISCC -and (Test-Path -LiteralPath $env:AGENT_NOTIFY_ISCC)) {
    return $env:AGENT_NOTIFY_ISCC
  }
  $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
  if ($command) { return $command.Source }
  foreach ($candidate in @(
      'D:\Temp\InnoSetup\ISCC.exe',
      "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
      "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    )) {
    if ($candidate -and (Test-Path -LiteralPath $candidate)) { return $candidate }
  }
  return $null
}

$iscc = Resolve-Iscc
if (-not $iscc) {
  throw '找不到 ISCC.exe。请安装 Inno Setup 6，或设置 AGENT_NOTIFY_ISCC。'
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$isccArgs = @(
  "/DAppVersion=$Version",
  "/DRepoRoot=$RepoRoot",
  "/DOutputDir=$OutDir",
  "/DExePath=$ExePath"
)
if ($env:AGENT_NOTIFY_SIGNTOOL) {
  $isccArgs += "/DSignToolCommand=1"
  $isccArgs += "/Sagentnotify=$env:AGENT_NOTIFY_SIGNTOOL sign `$f"
}
& $iscc @isccArgs (Join-Path $RepoRoot 'installer\agent-notify.iss')
if ($LASTEXITCODE -ne 0) {
  throw "Inno Setup 构建失败 exit=$LASTEXITCODE"
}
$installer = Join-Path $OutDir "Agent-notify-Setup-v$Version.exe"
if (-not (Test-Path -LiteralPath $installer)) {
  throw "安装器未生成：$installer"
}
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tests\installer-smoke.ps1') -Installer $installer
if ($LASTEXITCODE -ne 0) { throw '安装器结构检查失败' }
Write-Output "[installer] $installer"
```

在 `installer/agent-notify.iss` 的 `[Setup]` 区域增加条件签名：

```iss
#ifdef SignToolCommand
SignTool=agentnotify
#endif
```

签名参数已包含在上面的 `$isccArgs` 构建中；未设置 `AGENT_NOTIFY_SIGNTOOL` 时构建未签名安装器。

- [ ] **Step 5: 让发布构建同时生成 ZIP 和 Setup EXE**

在 `tools/build-release.ps1` 构建完 ZIP 后调用：

```powershell
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $RepoRoot 'tools\build-installer.ps1') -Version $Version -OutDir $OutDir -ExePath $tempExe
if ($LASTEXITCODE -ne 0) { throw "安装器构建失败 exit=$LASTEXITCODE" }
```

将 SHA256 汇总改为：

```powershell
$artifacts = @(
  (Get-ChildItem -LiteralPath $OutDir -File -Filter "Agent-notify-v$Version.zip"),
  (Get-ChildItem -LiteralPath $OutDir -File -Filter "Agent-notify-Setup-v$Version.exe")
)
$lines = foreach ($artifact in $artifacts) {
  $hash = (Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash.ToLower()
  "$hash  $($artifact.Name)"
}
$sumPath = Join-Path $OutDir 'SHA256SUMS.txt'
[IO.File]::WriteAllLines($sumPath, $lines, [Text.Encoding]::ASCII)
```

- [ ] **Step 6: 更新 Release workflow**

在安装依赖后加入 Inno Setup 安装步骤，将工具放在 `RUNNER_TEMP`：

```yaml
      - name: Install Inno Setup
        shell: powershell
        run: |
          $dir = Join-Path $env:RUNNER_TEMP 'InnoSetup'
          $installer = Join-Path $env:RUNNER_TEMP 'innosetup.exe'
          Invoke-WebRequest 'https://jrsoftware.org/download.php/is.exe' -OutFile $installer -UseBasicParsing
          Start-Process $installer -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=$dir") -Wait
          "AGENT_NOTIFY_ISCC=$dir\ISCC.exe" | Out-File $env:GITHUB_ENV -Append -Encoding ascii
```

发布步骤同时上传两个产物：

```powershell
$setup = (Get-ChildItem dist\Agent-notify-Setup-v*.exe | Select-Object -First 1).FullName
$zip = (Get-ChildItem dist\Agent-notify-v*.zip | Select-Object -First 1).FullName
gh release create $tag "$setup" "$zip" dist\SHA256SUMS.txt --title "Agent-notify $tag" --notes-file release-notes.md
```

- [ ] **Step 7: 本机构建并验证**

Run:

```powershell
$env:AGENT_NOTIFY_ISCC = 'D:\Temp\InnoSetup\ISCC.exe'
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build-release.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -Installer .\dist\Agent-notify-Setup-v1.4.1.exe
```

Expected: PASS，`dist` 同时存在 Setup EXE、ZIP 和包含两项哈希的 `SHA256SUMS.txt`。

- [ ] **Step 8: 提交**

```powershell
git add installer/agent-notify.iss tools/build-installer.ps1 tools/build-release.ps1 .github/workflows/release.yml tests/installer-smoke.ps1
git commit -m "feat(release): 生成标准 Windows 安装器"
```

---

### Task 5: 让升级按钮优先下载安装器

**Files:**
- Modify: `internal/update/update.go`
- Modify: `internal/update/launch.go`
- Modify: `internal/update/update_test.go`
- Modify: `internal/ui/update.go:80-130`

**Interfaces:**

```go
type ArtifactKind string

const (
	ArtifactInstaller ArtifactKind = "installer"
	ArtifactArchive   ArtifactKind = "archive"
)

type Release struct {
	Version      string
	TagName      string
	Notes        string
	ArtifactKind ArtifactKind
	ArtifactURL  string
	ChecksumURL  string
}

type PreparedUpdate struct {
	Version      string
	StageDir     string
	ArtifactPath string
	InstallerPath string
	Kind         ArtifactKind
}
```

- [ ] **Step 1: 写安装器资产选择测试**

在 `internal/update/update_test.go` 的 `TestCheckFindsNewerRelease` assets 中加入：

```go
{"name":"Agent-notify-Setup-v1.4.0.exe","url":"%s/setup"},
```

增加断言：

```go
if release.ArtifactKind != ArtifactInstaller || release.ArtifactURL != server.URL+"/setup" {
	t.Fatalf("installer artifact = %+v", release)
}
```

增加 ZIP 回退测试：

```go
func TestCheckFallsBackToZipWhenSetupMissing(t *testing.T) {
	// assets 只包含 Agent-notify-v1.4.0.zip 与 SHA256SUMS.txt
	// 断言 release.ArtifactKind == ArtifactArchive
}
```

- [ ] **Step 2: 运行测试并确认失败**

Run:

```powershell
go test ./internal/update -run TestCheckFindsNewerRelease -v
```

Expected: FAIL，`ArtifactKind` 不存在。

- [ ] **Step 3: 扩展 Release 和资产选择**

在 `internal/update/update.go` 加入类型：

```go
type ArtifactKind string

const (
	ArtifactInstaller ArtifactKind = "installer"
	ArtifactArchive   ArtifactKind = "archive"
)
```

把 `Release` 的 `ArchiveURL` 改为：

```go
	ArtifactKind ArtifactKind `json:"artifactKind"`
	ArtifactURL  string       `json:"artifactURL"`
```

在 `Check` 中先选择：

```go
setupName := fmt.Sprintf("Agent-notify-Setup-v%s.exe", version)
if setup, ok := findAsset(latest.Assets, setupName); ok {
	artifactKind = ArtifactInstaller
	artifactURL = assetDownloadURL(setup)
} else {
	archiveName := fmt.Sprintf("Agent-notify-v%s.zip", version)
	archive, ok := findAsset(latest.Assets, archiveName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 缺少更新包：%s", archiveName)
	}
	artifactKind = ArtifactArchive
	artifactURL = assetDownloadURL(archive)
}
```

增加：

```go
func assetDownloadURL(asset githubAsset) string {
	if value := strings.TrimSpace(asset.URL); value != "" {
		return value
	}
	return strings.TrimSpace(asset.BrowserDownloadURL)
}
```

公开 Release 跳转回退无法读取资产列表，因此继续选择 ZIP，并返回：

```go
return Release{
	Version:      version,
	TagName:      tagName,
	ArtifactKind: ArtifactArchive,
	ArtifactURL:  downloadBase + url.PathEscape(archiveName),
	ChecksumURL:  downloadBase + url.PathEscape(checksumName),
}, true, nil
```

- [ ] **Step 4: 准备安装器时不解压，ZIP 保持旧逻辑**

在 `Prepare` 中按 `ArtifactKind` 分支：

```go
artifactName := filepath.Base(release.ArtifactURL)
artifactPath := filepath.Join(stageDir, artifactName)
if err := client.download(ctx, release.ArtifactURL, artifactPath, maxArchiveBytes); err != nil {
	return PreparedUpdate{}, fmt.Errorf("下载更新包失败：%w", err)
}
if err := verifyChecksum(checksumsPath, artifactName, artifactPath); err != nil {
	return PreparedUpdate{}, err
}
if release.ArtifactKind == ArtifactInstaller {
	if err := validateWindowsExecutable(artifactPath); err != nil {
		return PreparedUpdate{}, err
	}
	return PreparedUpdate{
		Version:       version,
		StageDir:      stageDir,
		ArtifactPath:  artifactPath,
		InstallerPath: artifactPath,
		Kind:          ArtifactInstaller,
	}, nil
}
```

为 ZIP 保留现有 `extractZip`、`validatePreparedRelease` 和返回结构，并设置 `Kind: ArtifactArchive`、`ArtifactPath: archivePath`。

增加 PE 校验：

```go
func validateWindowsExecutable(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	header := make([]byte, 2)
	if _, err := io.ReadFull(file, header); err != nil || header[0] != 'M' || header[1] != 'Z' {
		return errors.New("更新安装器不是有效的 Windows 程序")
	}
	return nil
}
```

- [ ] **Step 5: 直接启动安装器，保留 ZIP 隐藏安装器**

在 `PreparedUpdate.Launch` 开头增加：

```go
if prepared.Kind == ArtifactInstaller {
	command := exec.Command(prepared.ArtifactPath, append([]string{"/NORESTART"}, installerArgs...)...)
	command.Dir = filepath.Dir(prepared.ArtifactPath)
	if err := command.Start(); err != nil {
		return fmt.Errorf("启动更新安装器失败：%w", err)
	}
	return nil
}
```

该分支不关闭当前悬浮窗。Inno Setup 的 `CloseApplications=yes` 会在真正开始安装时关闭旧进程，用户取消安装时应用继续运行。

- [ ] **Step 6: 调整 UI 成功后的退出行为**

在 `internal/ui/update.go` 中让 `startUpdateInstall` 根据类型决定是否主动退出：

```go
if err == nil {
	logPath := filepath.Join(root, "last-update.log")
	err = prepared.Launch(logPath, app.updateInstallerArgs()...)
}
if err != nil {
	app.finishUpdateInstall(err.Error())
	pPostMessageW.Call(hwnd, WM_USER_UPDATE_ERROR, 0, 0)
	return
}
if prepared.Kind == update.ArtifactInstaller {
	app.finishUpdateInstall("")
	pInvalidateRect.Call(hwnd, 0, 0)
	return
}
pPostMessageW.Call(hwnd, WM_USER_UPDATE_RESTART, 0, 0)
```

同时把升级按钮忙碌文案改为：

```go
case "installing":
	return "准备安装", false
```

- [ ] **Step 7: 运行更新模块测试**

Run:

```powershell
go test ./internal/update ./internal/ui -v
```

Expected: PASS；安装器优先、ZIP 回退、SHA256 失败和参数生成均有覆盖。

- [ ] **Step 8: 提交**

```powershell
git add internal/update internal/ui/update.go
git commit -m "feat(update): 优先使用安装器升级"
```

---

### Task 6: 更新文档、发布说明和完整验收

**Files:**
- Modify: `README.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/TROUBLESHOOTING.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: Setup EXE、首次启动初始化、升级按钮和卸载流程
- Produces: 与真实发布包一致的安装、升级、排障文档

- [ ] **Step 1: 改写 README 安装入口**

把“解压后运行 install.ps1”改为主流程：

```markdown
## 安装

从 Release 下载 `Agent-notify-Setup-vX.Y.Z.exe`，双击后按向导完成安装。安装完成页默认勾选“启动 Agent-notify”。

首次启动会自动完成 OpenCode、Codex、Antigravity、Devin 的接入。没有微信凭据时会打开 ClawBot 扫码窗口。

ZIP 包仅用于便携运行和开发调试。源码安装仍可使用：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```
```

- [ ] **Step 2: 更新架构和排障文档**

在 `docs/ARCHITECTURE.md` 增加：

- `installer/agent-notify.iss` 的职责
- 首次启动 `setup-state.json` 的作用
- 升级优先下载 Setup EXE，ZIP 作为兼容回退

在 `docs/TROUBLESHOOTING.md` 增加：

```markdown
### 双击安装器后 Agent 仍未接入

1. 完全退出 Agent-notify，再重新打开。
2. 点击悬浮窗底部“检查修复”。
3. 查看 `%TEMP%\agent-notify\setup.log`。
4. 如果安装器未创建开始菜单入口，重新运行最新版 `Agent-notify-Setup-v<版本>.exe`。
```

- [ ] **Step 3: 写入 Unreleased 变更**

在 `CHANGELOG.md` 的 `[Unreleased]` 中加入：

```markdown
### Added

- 新增标准 Windows 安装器 `Agent-notify-Setup-v<版本>.exe`，普通用户无需解压 ZIP 或运行 PowerShell。
- 首次启动自动静默完成 OpenCode、Codex、Antigravity、Devin 接入，失败后可从悬浮窗重试。

### Changed

- 应用内升级优先下载并校验新版安装器，安装完成后自动重启；旧 Release 缺少安装器时继续兼容 ZIP。
```

- [ ] **Step 4: 运行全部静态与单元测试**

Run:

```powershell
go test ./...
go vet ./...
gofmt -l cmd internal
.\node_modules\.bin\tsc.cmd --noEmit
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\test.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\lint.ps1
```

Expected: 全部通过，`gofmt -l` 无输出。

- [ ] **Step 5: 构建真实发布产物**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\build-release.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\installer-smoke.ps1 -Installer ".\dist\Agent-notify-Setup-v$(Get-Content .\VERSION -Raw).Trim().exe"
```

Expected: Setup EXE、ZIP 和 `SHA256SUMS.txt` 均生成，哈希一致。

- [ ] **Step 6: 在干净 Windows 用户环境做真实验收**

手动执行并记录结果：

1. 双击 Setup EXE，确认无命令行窗口、无管理员提示。
2. 在完成页点击启动，确认首次接入完成后打开悬浮窗。
3. 验证 OpenCode、Codex、Antigravity、Devin 的接入状态。
4. 使用旧版 ZIP 安装环境点击“升级”，确认新版 Setup EXE 下载、校验、启动安装并自动重启。
5. 从“已安装的应用”卸载，确认快捷方式和 Hook 被清理，用户凭据和历史仍存在。

- [ ] **Step 7: 提交**

```powershell
git add README.md docs/ARCHITECTURE.md docs/TROUBLESHOOTING.md CHANGELOG.md
git commit -m "docs(install): 更新标准安装和升级说明"
```

## Plan Self-Review

- 规格中的 Setup EXE、当前用户安装、首次启动接入、升级按钮、ZIP 兼容和标准卸载均由 Task 1 至 Task 6 覆盖。
- 未使用 TBD、TODO 或“稍后补充”等占位描述。
- `Options`、`WidgetOptions`、`ArtifactKind` 和 `PreparedUpdate` 的字段在后续任务中保持一致。
- 每个任务都包含先失败、再实现、再验证和独立提交步骤。
