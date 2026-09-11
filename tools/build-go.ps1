#Requires -Version 5.1
<#
.SYNOPSIS
  Go 原生编译构建脚本：编译 linkweixin.exe 单文件 + 打包 release zip + SHA256 校验。

.DESCRIPTION
  阶段 2 专用构建入口。
  - 运行 go test 全量单测
  - 编译 Windows GUI 原生二进制（-H windowsgui 无黑框控制台，-s -w 精简体积）
  - 打包为 linkWeixin-go-v<版本>.zip（内含单 exe + README + LICENSE + CHANGELOG）
  - 输出 SHA256SUMS.txt 校验文件

.PARAMETER Version
  指定版本号。默认从 internal/ui/widget.go 的 AppVersion 常量提取。

.PARAMETER OutDir
  输出目录。默认 dist/

.PARAMETER SkipTest
  跳过 go test（适用于 CI 已单独运行测试的场景）。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-go.ps1
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-go.ps1 -Version 0.4.0
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-go.ps1 -SkipTest
#>
param(
  [string]$Version,
  [string]$OutDir,
  [switch]$SkipTest
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent

# 1. Check Go installation
$goExe = Get-Command go -ErrorAction SilentlyContinue
if (-not $goExe) { throw "[build-go] Go 编译器未找到，请先安装 Go (https://go.dev/dl/)。" }
$goVer = & go version
Write-Output "[build-go] 使用 $goVer"

# 2. Extract version from source if not specified
if (-not $Version) {
  $widgetFile = Join-Path $RepoRoot 'internal\ui\widget.go'
  $m = Select-String -Path $widgetFile -Pattern 'AppVersion\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $m) { throw "[build-go] 无法从 widget.go 读出 AppVersion" }
  $Version = $m.Matches[0].Groups[1].Value
}
Write-Output "[build-go] 版本号: v$Version"

if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }

# 3. Run tests
if (-not $SkipTest) {
  Write-Output "[build-go] 运行 go test ./..."
  Push-Location $RepoRoot
  try {
    & go test -count=1 -v ./...
    if ($LASTEXITCODE -ne 0) { throw "[build-go] go test 失败 (exit=$LASTEXITCODE)" }
    Write-Output "[build-go] 全量测试通过 ✓"
  } finally {
    Pop-Location
  }
}

# 4. Build binary
$binDir = Join-Path $RepoRoot 'bin'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
$exePath = Join-Path $binDir 'linkweixin.exe'
Remove-Item $exePath -Force -ErrorAction SilentlyContinue

Write-Output "[build-go] 编译 linkweixin.exe (windowsgui, stripped)..."
$ldflags = "-H windowsgui -s -w -X linkweixin/internal/ui.AppVersion=$Version"
Push-Location $RepoRoot
try {
  & go build -ldflags $ldflags -trimpath -o $exePath .\cmd\linkweixin\
  if ($LASTEXITCODE -ne 0) { throw "[build-go] go build 失败" }
} finally {
  Pop-Location
}

$exeSize = (Get-Item $exePath).Length
$exeSizeMB = [math]::Round($exeSize / 1MB, 2)
Write-Output "[build-go] 编译完成: $exePath ($exeSizeMB MB)"

# 5. Verify binary runs (--version)
$verOut = (& $exePath version | Out-String).Trim()
Write-Output "[build-go] 版本验证: $verOut"

# 6. Package release zip
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stage = Join-Path $env:TEMP ("linkweixin-go-release-" + [guid]::NewGuid().ToString('N'))
$stageRoot = Join-Path $stage 'linkWeixin'
New-Item -ItemType Directory -Force -Path $stageRoot | Out-Null

try {
  Copy-Item $exePath $stageRoot -Force
  foreach ($f in @('LICENSE', 'README.md', 'CHANGELOG.md', '.env.example')) {
    $p = Join-Path $RepoRoot $f
    if (Test-Path $p) { Copy-Item $p $stageRoot -Force }
  }

  $zipName = "linkWeixin-go-v$Version.zip"
  $zipPath = Join-Path $OutDir $zipName
  Remove-Item $zipPath -Force -ErrorAction SilentlyContinue
  Compress-Archive -Path $stageRoot -DestinationPath $zipPath -CompressionLevel Optimal

  $hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
  $sumPath = Join-Path $OutDir 'SHA256SUMS.txt'
  "$hash  $zipName" | Out-File -FilePath $sumPath -Encoding ascii -Force

  $zipSize = [math]::Round((Get-Item $zipPath).Length / 1MB, 2)
  Write-Output ""
  Write-Output "=========================================="
  Write-Output "[build-go] 构建成功 ✓"
  Write-Output "  exe:  $exePath ($exeSizeMB MB)"
  Write-Output "  zip:  $zipPath ($zipSize MB)"
  Write-Output "  sha:  $sumPath"
  Write-Output "  hash: $hash"
  Write-Output "=========================================="
} finally {
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}
