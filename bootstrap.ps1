[CmdletBinding()]
param(
    [string]$Workspace = (Get-Location).Path,
    [string]$Model = "auto",
    [string]$Repository = $(if ($env:EVERYTHING_GITHUB_REPOSITORY) { $env:EVERYTHING_GITHUB_REPOSITORY } else { "brutalstein/everything" }),
    [switch]$NoLaunch,
    [switch]$NoService,
    [switch]$NoVerify
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
    throw "GitHub depo adı KULLANICI/REPO biçiminde olmalıdır: $Repository"
}
$ReleaseBase = if ($env:EVERYTHING_RELEASE_BASE_URL) { $env:EVERYTHING_RELEASE_BASE_URL.TrimEnd('/') } else { "https://github.com/$Repository/releases/latest/download" }
$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("everything-bootstrap-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null
try {
    $Asset = Join-Path $TempRoot "everything-source.zip"
    $Checksum = Join-Path $TempRoot "everything-source.zip.sha256"
    Write-Host "[Everything Bootstrap] Son doğrulanmış kaynak sürümü indiriliyor: $Repository" -ForegroundColor Cyan
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseBase/everything-source.zip" -OutFile $Asset -MaximumRedirection 10
    Invoke-WebRequest -UseBasicParsing -Uri "$ReleaseBase/everything-source.zip.sha256" -OutFile $Checksum -MaximumRedirection 10

    Write-Host "[Everything Bootstrap] SHA-256 özeti doğrulanıyor" -ForegroundColor Cyan
    $Expected = ((Get-Content $Checksum -TotalCount 1) -split '\s+')[0].Trim().ToLowerInvariant()
    if ($Expected -notmatch '^[0-9a-f]{64}$') { throw "Release SHA-256 dosyası geçersiz." }
    $Actual = (Get-FileHash -Algorithm SHA256 -Path $Asset).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw "Release SHA-256 doğrulaması başarısız. İndirilen dosya kullanılmayacak." }

    $SourceDir = Join-Path $TempRoot "source"
    Expand-Archive -Path $Asset -DestinationPath $SourceDir -Force
    $Setup = Join-Path $SourceDir "setup.ps1"
    if (-not (Test-Path $Setup)) { throw "Release arşivinde setup.ps1 bulunamadı." }

    Write-Host "[Everything Bootstrap] Doğrulanan kurucu başlatılıyor" -ForegroundColor Cyan
    & $Setup -Workspace $Workspace -Model $Model -NoLaunch:$NoLaunch -NoService:$NoService -NoVerify:$NoVerify
} finally {
    Remove-Item $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
