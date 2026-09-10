#Requires -Version 5.1
<#
  仓库守卫测试：编码/行尾硬门禁。
  .editorconfig 只约束编辑器，这里才是 CI 的硬检查（README 记录的踩坑 #3）。
#>

BeforeAll {
  $RepoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
  $RepoFiles = @(Get-ChildItem -Path $RepoRoot -Recurse -File -Include *.ps1, *.psm1, *.psd1 |
      Where-Object { $_.FullName -notmatch '\\\.git\\|\\node_modules\\|\\dist\\' })
}

Describe '仓库守卫' {
  It '所有 ps1/psm1/psd1 带 UTF-8 BOM' {
    $bad = @()
    foreach ($f in $RepoFiles) {
      $b = [IO.File]::ReadAllBytes($f.FullName)
      $hasBom = ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
      if (-not $hasBom) { $bad += $f.FullName }
    }
    ($bad -join "`n") | Should -BeNullOrEmpty
  }
  It '所有 ps1/psm1/psd1 为 CRLF 行尾' {
    $bad = @()
    foreach ($f in $RepoFiles) {
      $txt = [IO.File]::ReadAllText($f.FullName)
      if ($txt -match "(?<!`r)`n") { $bad += $f.FullName }
    }
    ($bad -join "`n") | Should -BeNullOrEmpty
  }
}
