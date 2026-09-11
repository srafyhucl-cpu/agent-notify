$bom = [byte[]](0xEF, 0xBB, 0xBF)
$files = Get-ChildItem -Path (Split-Path $PSScriptRoot -Parent) -Recurse -File -Include *.ps1, *.psm1, *.psd1 |
    Where-Object { $_.FullName -notmatch '\\\.git\\|\\node_modules\\|\\dist\\' }

foreach ($f in $files) {
    $b = [System.IO.File]::ReadAllBytes($f.FullName)
    $hasBom = ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
    if (-not $hasBom) {
        $nb = $bom + $b
        [System.IO.File]::WriteAllBytes($f.FullName, $nb)
        Write-Output "BOM added: $($f.FullName)"
    }
    $txt = [System.IO.File]::ReadAllText($f.FullName)
    if ($txt -match "(?<!`r)`n") {
        $txt = $txt -replace "(?<!`r)`n", "`r`n"
        [System.IO.File]::WriteAllText($f.FullName, $txt, [System.Text.Encoding]::UTF8)
        Write-Output "CRLF fixed: $($f.FullName)"
    }
}
Write-Output "All files verified for BOM and CRLF."
