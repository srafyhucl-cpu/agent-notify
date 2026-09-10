function Get-CodexComputerUseExe {
  <#
  .SYNOPSIS
    动态定位最新的 codex-computer-use.exe（路径里的哈希目录会随 codex 更新变化）。
    找不到返回 $null，绝不抛异常。
  #>
  [CmdletBinding()]
  param([string]$LocalAppData = $env:LOCALAPPDATA)

  try {
    Get-ChildItem "$LocalAppData\OpenAI\Codex\runtimes\cua_node\*\bin\node_modules\@oai\sky\bin\windows\codex-computer-use.exe" -ErrorAction Stop |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1 -ExpandProperty FullName
  } catch { $null }
}
