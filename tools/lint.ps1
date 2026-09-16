#Requires -Version 5.1
<#
.SYNOPSIS
  PowerShell 静态检查：语法解析全量扫描 + PSScriptAnalyzer（可用时）Error 级检查。

.DESCRIPTION
  语法解析不依赖外部模块，始终执行；PSScriptAnalyzer 若已安装则额外跑一遍
  Error 级规则。CI 与本机共用同一入口。

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\lint.ps1
#>
param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path $PSScriptRoot -Parent
$failures = @()

$files = @(Get-ChildItem -Path $RepoRoot -Recurse -File -Include *.ps1, *.psm1, *.psd1 |
    Where-Object { $_.FullName -notmatch '[\\/](\.git|node_modules|dist|bin)[\\/]' })

foreach ($file in $files) {
  $tokens = $null
  $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors)
  if ($errors.Count -gt 0) {
    $failures += "$($file.Name):$($errors[0].Extent.StartLineNumber) $($errors[0].Message)"
  }
}

# GitHub Actions 的 run 脚本会被写成无 BOM 临时文件，PS 5.1 按 ANSI 读取：workflow 里出现中文会破坏引号导致语法错误。
$workflowDir = Join-Path $RepoRoot '.github\workflows'
if (Test-Path -LiteralPath $workflowDir) {
  foreach ($file in @(Get-ChildItem -LiteralPath $workflowDir -File -Include *.yml, *.yaml)) {
    $nonAsciiLine = Select-String -Path $file.FullName -Pattern '[^\x00-\x7F]' | Select-Object -First 1
    if ($nonAsciiLine) {
      $failures += "$($file.Name):$($nonAsciiLine.LineNumber) workflow 含非 ASCII 字符，PS 5.1 解析 run 脚本会乱码，请改用 ASCII"
    }
  }
}

if ($failures.Count -gt 0) {
  $failures | ForEach-Object { Write-Output "[lint] 语法错误：$_" }
  exit 1
}
Write-Output "[lint] 语法解析通过（$($files.Count) 个文件）"

if (Get-Module PSScriptAnalyzer -ListAvailable) {
  Import-Module PSScriptAnalyzer
  $results = @(Invoke-ScriptAnalyzer -Path $RepoRoot -Recurse -Severity Error |
      Where-Object { $_.ScriptPath -notmatch '[\\/](\.git|node_modules|dist|bin)[\\/]' })
  if ($results.Count -gt 0) {
    foreach ($result in $results) {
      Write-Output ("[lint] {0}  {1}:{2}  {3}" -f $result.Severity, $result.ScriptName, $result.Line, $result.Message)
    }
    exit 1
  }
  Write-Output '[lint] PSScriptAnalyzer Error 级检查通过'
} else {
  Write-Output '[lint] 未安装 PSScriptAnalyzer，跳过深度规则检查'
}

# 版本号一致性：与 release workflow 的 tag 校验共用同一脚本
$versionChecker = Join-Path $PSScriptRoot 'check-version.ps1'
if (Test-Path -LiteralPath $versionChecker -PathType Leaf) {
  if ($PSVersionTable.PSVersion.Major -ge 6) {
    & pwsh -NoProfile -File $versionChecker
  } else {
    & powershell -NoProfile -ExecutionPolicy Bypass -File $versionChecker
  }
  if ($LASTEXITCODE -ne 0) {
    Write-Output '[lint] 版本号一致性检查失败'
    exit 1
  }
}

exit 0
