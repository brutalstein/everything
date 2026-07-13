$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Operation,
        [Parameter(Mandatory = $true)][string]$Description
    )
    Write-Host "`n[Everything Doğrulama] $Description" -ForegroundColor Cyan
    & $Operation
    if ($LASTEXITCODE -ne 0) { throw "$Description başarısız oldu (çıkış kodu: $LASTEXITCODE)." }
}

function Find-Python {
    foreach ($name in @("python", "python.exe")) {
        $command = Get-Command $name -ErrorAction SilentlyContinue
        if ($command -and $command.Source -notmatch '\\WindowsApps\\') {
            try {
                & $command.Source -c "import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)"
                if ($LASTEXITCODE -eq 0) { return $command.Source }
            } catch {}
        }
    }
    $launcher = Get-Command py.exe -ErrorAction SilentlyContinue
    if ($launcher) {
        try {
            $path = (& $launcher.Source -3.12 -c "import sys; print(sys.executable)" | Select-Object -First 1).Trim()
            if ($LASTEXITCODE -eq 0 -and $path) { return $path }
        } catch {}
    }
    throw "Python 3.11+ bulunamadı. Önce setup.ps1 çalıştırın."
}

$Python = Find-Python
$Archive = Join-Path $Root "everything-source.zip"

Push-Location $Root
try {
    Invoke-Checked { cargo fmt --all -- --check } "Rust biçim kontrolü"
    Invoke-Checked { cargo clippy --locked --workspace --all-targets -- -D warnings } "Rust Clippy denetimi"
    Invoke-Checked { cargo test --locked --workspace --all-targets } "Rust testleri"
    Invoke-Checked { cargo build --locked --workspace --release } "Rust release derlemesi"

    Push-Location (Join-Path $Root "apps/everything-app")
    try {
        Invoke-Checked { npm ci } "Electron bağımlılık kurulumu"
        Invoke-Checked { npm run typecheck } "Electron TypeScript kontrolü"
        Invoke-Checked { npm run build } "Electron derlemesi"
        Invoke-Checked { npm audit --omit=dev --audit-level=high } "Electron üretim güvenlik denetimi"
    }
    finally { Pop-Location }

    $VerifyVenv = Join-Path ([System.IO.Path]::GetTempPath()) ("everything-verify-python-" + [Guid]::NewGuid().ToString("N"))
    try {
        Invoke-Checked { & $Python -m venv $VerifyVenv } "Python doğrulama ortamı oluşturma"
        $VerifyPython = Join-Path $VerifyVenv "Scripts/python.exe"
        Invoke-Checked { & $VerifyPython -m pip install --upgrade pip } "pip güncelleme"
        Invoke-Checked { & $VerifyPython -m pip install -e ((Join-Path $Root "python/everything_control") + "[dev]") tree-sitter tree-sitter-rust } "Python ve statik analiz bağımlılık kurulumu"
        Push-Location (Join-Path $Root "python/everything_control")
        try {
            Invoke-Checked { & $VerifyPython -m pytest } "Python SDK testleri"
            Invoke-Checked { & $VerifyPython -m build } "Python SDK paketleme"
        }
        finally { Pop-Location }
        Push-Location $Root
        try {
            Invoke-Checked { & $VerifyPython scripts/static_rust_check.py } "Rust statik sözleşme kontrolü"
        }
        finally { Pop-Location }
    }
    finally {
        if (Test-Path $VerifyVenv) { Remove-Item -Recurse -Force $VerifyVenv }
    }

    Invoke-Checked { & $Python scripts/check_versions.py } "Proje sürüm eşleşmesi"
    Invoke-Checked { & $Python scripts/smoke_installers.py } "Kurucu sözleşme testi"
    Invoke-Checked { & $Python scripts/smoke_mvp.py --require-built-ui } "Statik ürün duman testi"
    Invoke-Checked { & $Python scripts/package_source.py --output $Archive } "Deterministik kaynak arşivi oluşturma"
    Invoke-Checked { & $Python scripts/validate_source_archive.py $Archive } "Kaynak arşivi doğrulama"

    Write-Host "`n[Everything Doğrulama] TÜM KONTROLLER BAŞARILI" -ForegroundColor Green
    Write-Host "Kaynak arşivi: $Archive"
}
finally { Pop-Location }
