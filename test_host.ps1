# File: test_host.ps1
# minOs host birim testleri (test_host.ps1)
#
# Neden ayri bir betik?
# ---------------------
# Cekirdek `x86_64-minos.json` hedefine `panic = "abort"` + `-Z build-std` ile
# derlenir. `cargo test` ise `--test` modu yuzunden `panic = "unwind"` ister;
# Cargo bu durumda `core`'u ikinci bir hash ile yeniden derler ve rustc
# "duplicate lang item in crate `core`" (E0152) hatasini verir.
#
# Bu betik, cekirdekten bagimsiz saf mantik modullerini (src/drivers/crypto)
# `tests/host_tests.rs` uzerinden HOST hedefi icin derleyip calistirir.
# Cekirdege ait build-std yapilandirmasi devre disi birakilir.

param(
    [switch]$NoRun
)

$ErrorActionPreference = "Continue" # native araclarin stderr ciktisi betigi durdurmasin

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $repoRoot

$outDir = Join-Path $repoRoot "target\host-tests"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$harness = Join-Path $repoRoot "tests\host_tests.rs"
$binary  = Join-Path $outDir "host_tests.exe"

if (-not (Test-Path $harness)) {
    Write-Host "[HATA] Test harness bulunamadi: $harness" -ForegroundColor Red
    exit 1
}



# --target explicit olarak host'a sabitlenir; .cargo/config.toml icindeki
# `[build] target` ve `-Z build-std` ayarlari boylece uygulanmaz.
$hostTriple = (& rustc --version --verbose | Select-String '^host: ').Line -replace '^host: ', ''

# rustc also reads Cargo configuration from the environment. In particular,
# CARGO_ENCODED_RUSTFLAGS can inject the kernel linker script and target flags
# into this host build. Use an isolated Cargo home and clear Cargo's per-process
# overrides so the host compiler sees only its own standard library.
$hostCargoHome = Join-Path $outDir "cargo-home"
New-Item -ItemType Directory -Force -Path $hostCargoHome | Out-Null
$emptyCargoConfig = Join-Path $hostCargoHome "config.toml"
Set-Content -LiteralPath $emptyCargoConfig -Value "" -Encoding Ascii
$savedCargoHome = $env:CARGO_HOME
$savedCargoTargetDir = $env:CARGO_TARGET_DIR
$savedCargoEncodedRustflags = $env:CARGO_ENCODED_RUSTFLAGS
$savedCargoBuildTarget = $env:CARGO_BUILD_TARGET
$env:CARGO_HOME = $hostCargoHome
$env:CARGO_TARGET_DIR = $null
$env:CARGO_ENCODED_RUSTFLAGS = $null
$env:CARGO_BUILD_TARGET = $null

Write-Host "[2/3] Host testleri derleniyor..." -ForegroundColor Yellow

# `--test` modunda rustc, `--extern` verilen crate'leri arar. `miniz_oxide` gibi
# Cargo bagimliliklari bu betikte Cargo tarafindan derlenmez (Cargo, depo kokundeki
# `.cargo/config.toml` icindeki `-Z build-std`'yi miras alip `core`'u yeniden
# derliyor ve E0152 duplicate lang item hatasi veriyor). Bunun yerine bagimlilik
# crate'leri (adler2, miniz_oxide) kaynaktan dogrudan rustc ile derlenir; boylece
# sysroot `core`/`alloc` kullanilir ve derleme tamamen deterministiktir.
$pkgDir = Join-Path $repoRoot "target\host-tests\deps"
New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
function Get-RegistrySrc {
    param([string]$Name, [string]$Version)
    $srcRoot = Join-Path $env:USERPROFILE ".cargo\registry\src"
    return Get-ChildItem -LiteralPath $srcRoot -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "$Name-$Version" } |
        Where-Object { Test-Path -LiteralPath (Join-Path $_ "Cargo.toml") } |
        Select-Object -First 1
}
# Surumler `Cargo.lock`'tan okunur; boylece bagimlilik guncellendiginde betik
# guncelleme gerektirmez.
function Get-LockedVersion {
    param([string]$Name)
    $lock = Get-Content -LiteralPath (Join-Path $repoRoot "Cargo.lock") -Raw
    $m = [regex]::Match($lock, '\[\[package\]\]\s*name = "' + [regex]::Escape($Name) + '"\s*version = "([^"]+)"')
    if (-not $m.Success) { throw "Cargo.lock icinde $Name bulunamadi" }
    return $m.Groups[1].Value
}
$minizSrc = Get-RegistrySrc -Name "miniz_oxide" -Version (Get-LockedVersion -Name "miniz_oxide")
$adlerSrc = Get-RegistrySrc -Name "adler2" -Version (Get-LockedVersion -Name "adler2")
if (-not $minizSrc -or -not $adlerSrc) {
    Write-Host "[HATA] miniz_oxide/adler2 kaynak dizini bulunamadi." -ForegroundColor Red
    exit 1
}

function Build-HostCrate {
    param([string]$Manifest, [string]$CrateName, [string[]]$ExtraArgs, [string]$Cfg)
    $rlib = Join-Path $pkgDir "lib$CrateName.rlib"
    $args = @("+nightly", "--edition", "2021", "--crate-type", "rlib", "--crate-name", $CrateName,
              $Manifest, "--target", $hostTriple, "-o", $rlib) + $ExtraArgs
    if ($Cfg) {
        # PowerShell native argument passing can strip the quotes from
        # `feature="with-alloc"` (or leave backslashes behind). Rustc response
        # files preserve the cfg expression exactly.
        $cfgFile = Join-Path $pkgDir "$CrateName.cfg.rsp"
        Set-Content -LiteralPath $cfgFile -Value "--cfg`n$Cfg" -Encoding Ascii
        $args += "@$cfgFile"
    }
    $hostOut = & rustc @args 2>&1
    if ($LASTEXITCODE -ne 0) {
        $hostOut | ForEach-Object { Write-Host $_ }
        throw "$CrateName derlenemedi"
    }
    # Adi sabit bir yola kopyalamak yetmiyor: metadata adi sabit oldugu icin
    # `--extern` farkli metadata tasiyan rlib'i reddediyor. `--extern`'e verilen
    # degeri adin = adiyla degil, tam dosya yolu ile gecirdigimiz icin sorun yok.
    return $rlib
}

Build-HostCrate -Manifest (Join-Path $adlerSrc "src\lib.rs") -CrateName "adler2" -ExtraArgs @()
$minizRlib = Build-HostCrate -Manifest (Join-Path $minizSrc "src\lib.rs") -CrateName "miniz_oxide" `
    -ExtraArgs @("-L", "dependency=$pkgDir", "--extern", "adler2=$(Join-Path $pkgDir 'libadler2.rlib')") `
    -Cfg 'feature="with-alloc"'
Write-Host "        miniz_oxide -> $minizRlib"

$rustcArgs = @(
    "+nightly", "--edition", "2021", "--test", $harness,
    "--target", $hostTriple,
    "-o", $binary,
    "--extern", "alloc",
    "--extern", "miniz_oxide=$minizRlib",
# miniz_oxide'nin kendi bagimliligi (transitif `-L` doyaylastirilamaz).
    "-L", "dependency=$pkgDir",
    "--cfg", "host_fs_tests"
)
$output = & rustc @rustcArgs 2>&1
$buildCode = $LASTEXITCODE
$env:CARGO_HOME = $savedCargoHome
$env:CARGO_TARGET_DIR = $savedCargoTargetDir
$env:CARGO_ENCODED_RUSTFLAGS = $savedCargoEncodedRustflags
$env:CARGO_BUILD_TARGET = $savedCargoBuildTarget
$output | ForEach-Object { Write-Host $_ } # rustc uyarilari stderr'e yazar; bunlar hata degil

if ($buildCode -ne 0 -or -not (Test-Path $binary)) {
    Write-Host "[HATA] Derleme basarisiz." -ForegroundColor Red
    exit 1
}

if ($NoRun) {
    Write-Host "[BILGI] --NoRun verildi; test calistirilmadi: $binary" -ForegroundColor Cyan
    exit 0
}

Write-Host "[3/3] Testler calistiriliyor..." -ForegroundColor Yellow
& $binary --test-threads=1
$code = $LASTEXITCODE

if ($code -ne 0) {
    Write-Host "[HATA] Test(ler) basarisiz (exit $code)." -ForegroundColor Red
    exit $code
}

Write-Host "[BASARILI] Host testleri gecti." -ForegroundColor Green
