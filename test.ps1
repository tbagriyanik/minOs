# File: test.ps1
# minOs tek giriş noktalı test kosucusu (test.ps1)
#
# Kullanim:
#   powershell -ExecutionPolicy Bypass -File .\test.ps1 -Mode Host
#   powershell -ExecutionPolicy Bypass -File .\test.ps1 -Mode Qemu
#   powershell -ExecutionPolicy Bypass -File .\test.ps1 -Mode Perf -MinScore 75
#
# Mode'a gore alt alta gelen ozelligi test betigini cagirir:
#   Host  -> .\test_host.ps1  (birim testleri, host hedefi)
#   Qemu  -> .\test_qemu.ps1  (QEMU entegrasyon ve boot dogrulama)
#   Perf  -> .\test_perf.ps1  (performans dogrulama)

param(
    [ValidateSet("Host", "Qemu", "Perf")]
    [string]$Mode = "Host",

    # Yalnızca Host modunda gecerli: test derlenir calistirilmaz.
    [switch]$NoRun
)

$ErrorActionPreference = "Continue"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "   minOs Test Kosucusu - Mod: $Mode   " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

switch ($Mode) {
    "Host" {
        # `-NoRun:$NoRun` biçimi powershell.exe -File üzerinden switch
        # parametresine "System.String" olarak ulaşabiliyor; koşullu
        # argüman dizisi güvenli.
        $hostArgs = @()
        if ($NoRun) { $hostArgs += "-NoRun" }
        & powershell -ExecutionPolicy Bypass -File ".\test_host.ps1" @hostArgs
    }
    # "Qemu" case removed as part of QEMU integration test removal
    "Perf" {
        & powershell -ExecutionPolicy Bypass -File ".\test_perf.ps1"
    }
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "`n[HATA] Test kosucu basarisiz (exit $LASTEXITCODE)." -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host "`n[BASARILI] Test kosucu tamamlandi (Mod: $Mode)." -ForegroundColor Green
