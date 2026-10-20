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

Write-Host "[1/2] Host testleri derleniyor..." -ForegroundColor Yellow

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

$rustcArgs = @(
    "+nightly", "--edition", "2021", "--test", $harness,
    "--target", $hostTriple,
    "-o", $binary,
    "--extern", "alloc"
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

Write-Host "[2/2] Testler calistiriliyor..." -ForegroundColor Yellow
& $binary --test-threads=1
$code = $LASTEXITCODE

if ($code -ne 0) {
    Write-Host "[HATA] Test(ler) basarisiz (exit $code)." -ForegroundColor Red
    exit $code
}

Write-Host "[BASARILI] Host testleri gecti." -ForegroundColor Green
