param([ValidateSet("start", "stop", "status")][string]$Action = "start")
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Compose = if ($env:EVERYTHING_SEARXNG_COMPOSE) { $env:EVERYTHING_SEARXNG_COMPOSE } else { Join-Path $Root "deploy\searxng\compose.yml" }
$Port = if ($env:EVERYTHING_SEARXNG_PORT) { $env:EVERYTHING_SEARXNG_PORT } else { "8888" }
$EverythingHome = if ($env:EVERYTHING_HOME) { $env:EVERYTHING_HOME } else { Join-Path $HOME ".everything" }
$StateDir = Join-Path $EverythingHome "research\searxng"
$Settings = if ($env:EVERYTHING_SEARXNG_SETTINGS) { $env:EVERYTHING_SEARXNG_SETTINGS } else { Join-Path $StateDir "settings.yml" }
$Project = if ($env:EVERYTHING_SEARXNG_PROJECT) { $env:EVERYTHING_SEARXNG_PROJECT } else { "everything-research" }

New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
if (-not (Test-Path $Settings)) {
    $template = Join-Path $Root "deploy\searxng\settings.yml"
    if (-not (Test-Path $template)) { throw "SearXNG settings template missing: $template" }
    $bytes = [byte[]]::new(32)
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    $secret = -join ($bytes | ForEach-Object { $_.ToString("x2") })
    $content = (Get-Content -Raw $template).Replace("everything-local-loopback-only", $secret)
    [System.IO.File]::WriteAllText($Settings, $content, [System.Text.UTF8Encoding]::new($false))
}
$env:EVERYTHING_SEARXNG_SETTINGS = $Settings
$env:EVERYTHING_SEARXNG_PORT = $Port
$env:COMPOSE_PROJECT_NAME = $Project

function Invoke-Compose([string[]]$Arguments) {
    if (Get-Command docker -ErrorAction SilentlyContinue) {
        & docker compose -p $Project -f $Compose @Arguments
        return $LASTEXITCODE
    }
    if (Get-Command podman -ErrorAction SilentlyContinue) {
        & podman compose -p $Project -f $Compose @Arguments
        return $LASTEXITCODE
    }
    return 127
}

if ($Action -eq "stop") { [void](Invoke-Compose @("down", "--remove-orphans")); exit 0 }
if ($Action -eq "status") { exit (Invoke-Compose @("ps")) }
$status = Invoke-Compose @("up", "-d", "--remove-orphans")
if ($status -eq 127) { Write-Warning "Docker/Podman bulunamadı; native anahtarsız web sağlayıcıları kullanılacak."; exit 0 }
if ($status -ne 0) { Write-Warning "Yerel SearXNG başlatılamadı; native anahtarsız web sağlayıcılarına geçiliyor."; exit 0 }
for ($attempt = 0; $attempt -lt 8; $attempt++) {
    try {
        Invoke-RestMethod -Uri "http://127.0.0.1:$Port/search?q=everything&format=json" -TimeoutSec 4 | Out-Null
        Write-Host "[everything] Yerel SearXNG hazır: http://127.0.0.1:$Port"
        exit 0
    } catch { Start-Sleep -Seconds ([Math]::Min(8, [Math]::Pow(2, [Math]::Min($attempt, 3)))) }
}
Write-Warning "SearXNG container başlatıldı; health-check henüz hazır değil."
