[CmdletBinding()]
param(
    [string]$Workspace = (Get-Location).Path,
    [string]$Model = $(if ($env:EVERYTHING_MODEL) { $env:EVERYTHING_MODEL } else { "auto" }),
    [switch]$NoLaunch,
    [switch]$NoService,
    [switch]$NoVerify
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = (Resolve-Path $PSScriptRoot).Path
$SetupStateDir = Join-Path $HOME ".everything\setup"
New-Item -ItemType Directory -Force -Path $SetupStateDir | Out-Null
$LogFile = Join-Path $SetupStateDir ("setup-{0}.log" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
$TranscriptStarted = $false

function Write-SetupStep([string]$Message) {
    Write-Host "`n[Everything Kurulum] $Message" -ForegroundColor Cyan
}

function Get-FreeDiskGiB {
    $probe = if ($env:OLLAMA_MODELS) { $env:OLLAMA_MODELS } else { Join-Path $HOME ".ollama" }
    $rootPath = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath($probe))
    return [math]::Floor(([System.IO.DriveInfo]::new($rootPath)).AvailableFreeSpace / 1GB)
}

try {
    if ($env:OS -ne "Windows_NT") {
        throw "Bu dosya yalnızca Windows içindir. Linux/macOS kullanıyorsanız setup.sh dosyasını çalıştırın."
    }
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw "PowerShell 5.1 veya daha yenisi gerekiyor."
    }

    try {
        Start-Transcript -Path $LogFile -Force | Out-Null
        $TranscriptStarted = $true
    } catch {
        Write-Warning "Kurulum günlüğü başlatılamadı; kurulum yine de devam edecek: $($_.Exception.Message)"
    }

    $Workspace = [System.IO.Path]::GetFullPath($Workspace)
    Write-SetupStep "Başlatılıyor"
    Write-Host "  Kaynak klasörü : $Root"
    Write-Host "  Çalışma klasörü: $Workspace"
    Write-Host "  Ayrıntılı günlük: $LogFile"

    if ($Model -eq "auto") {
        $memory = 0
        try { $memory = [math]::Floor((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB) } catch {}
        $gpu = 0
        if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
            foreach ($value in (& nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>$null)) {
                $parsed = 0
                if ([int]::TryParse(($value -replace '[^0-9]', ''), [ref]$parsed)) {
                    $gpu = [math]::Max($gpu, [math]::Floor($parsed / 1024))
                }
            }
        }
        $disk = Get-FreeDiskGiB
        if ($disk -lt 15) {
            throw "Derleme araçları, masaüstü uygulaması ve yerel model için en az 15 GiB boş alan gerekiyor. Algılanan boş alan: $disk GiB."
        }
        if ($gpu -ge 16 -and $memory -ge 24 -and $disk -ge 35) {
            $Model = "qwen2.5-coder:14b"
        } elseif (($gpu -ge 8 -or $memory -ge 16) -and $disk -ge 22) {
            $Model = "qwen2.5-coder:7b"
        } else {
            $Model = "qwen2.5-coder:3b"
        }
        Write-SetupStep "Sistem için uygun model seçildi: $Model"
        Write-Host "  RAM: $memory GiB | NVIDIA VRAM: $gpu GiB | Boş disk: $disk GiB"
    } else {
        Write-SetupStep "İstenen model kullanılacak: $Model"
    }

    $params = @{
        Workspace = $Workspace
        Model = $Model
        InstallDeps = $true
        PullModel = $true
        NoLaunch = $NoLaunch
        NoService = $NoService
        NoVerify = $NoVerify
    }
    & (Join-Path $Root "install.ps1") @params

    Write-SetupStep "Kurulum başarıyla tamamlandı"
    Write-Host "Everything komutu yeni terminallerde kullanılabilir."
    Write-Host "Kurulum günlüğü: $LogFile"
} catch {
    $stageFile = Join-Path $SetupStateDir "current-stage"
    $stage = if (Test-Path $stageFile) { (Get-Content $stageFile -Raw).Trim() } else { "başlangıç" }
    Write-Host "`n[Everything Kurulum] KURULUM BAŞARISIZ" -ForegroundColor Red
    Write-Host "Aşama : $stage" -ForegroundColor Yellow
    Write-Host "Hata  : $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Günlük: $LogFile" -ForegroundColor Yellow
    Write-Host "Sorunu düzelttikten sonra aynı setup.ps1 komutunu tekrar çalıştırabilirsiniz; kurucu kaldığı ortamı güvenli biçimde yeniden denetler." -ForegroundColor Yellow
    throw
} finally {
    if ($TranscriptStarted) {
        try { Stop-Transcript | Out-Null } catch {}
    }
}
