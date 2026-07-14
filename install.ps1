[CmdletBinding()]
param(
    [string]$Workspace = (Get-Location).Path,
    [string]$Model = "qwen2.5-coder:7b",
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Everything"),
    [switch]$InstallDeps,
    [switch]$PullModel,
    [switch]$NoVerify,
    [switch]$NoLaunch,
    [switch]$NoService
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Version = "0.3.0"
$RustToolchain = "1.97.0"
$Root = (Resolve-Path $PSScriptRoot).Path
$Workspace = [System.IO.Path]::GetFullPath($Workspace)
$BinDir = Join-Path $env:LOCALAPPDATA "Everything\bin"
$SetupStateDir = Join-Path $HOME ".everything\setup"
$PortStateFile = Join-Path $SetupStateDir "service-ports.env"
$ServicePortExplicit = [bool]$env:EVERYTHING_SERVICE_PORT
$OAuthPortExplicit = [bool]$env:EVERYTHING_OAUTH_PORT
$OriginalProcessPath = $env:Path
$PythonExe = $null
$PersistedPorts = @{}
if (Test-Path $PortStateFile) {
    foreach ($line in Get-Content $PortStateFile) {
        if ($line -match '^(SERVICE_PORT|OAUTH_PORT)=([0-9]+)$') { $PersistedPorts[$Matches[1]] = [int]$Matches[2] }
    }
}
$ServicePort = if ($ServicePortExplicit) { [int]$env:EVERYTHING_SERVICE_PORT } elseif ($PersistedPorts.ContainsKey('SERVICE_PORT')) { $PersistedPorts['SERVICE_PORT'] } else { 3472 }
$OAuthPort = if ($OAuthPortExplicit) { [int]$env:EVERYTHING_OAUTH_PORT } elseif ($PersistedPorts.ContainsKey('OAUTH_PORT')) { $PersistedPorts['OAUTH_PORT'] } else { 43821 }
foreach ($port in @($ServicePort, $OAuthPort)) {
    if ($port -lt 1024 -or $port -gt 65535) { throw "Servis portları 1024 ile 65535 arasında olmalıdır." }
}
if ($ServicePort -eq $OAuthPort) { throw "Servis portu ile OAuth geri dönüş portu farklı olmalıdır." }
$InstallBackup = "$InstallDir.previous"
$InstallSwitched = $false
$InstallComplete = $false
$ResearchSidecarStatus = "fallback"
$ShouldLaunch = -not $NoLaunch
New-Item -ItemType Directory -Force -Path $SetupStateDir | Out-Null
$InstallMutex = [System.Threading.Mutex]::new($false, "Local\EverythingInstaller")
if (-not $InstallMutex.WaitOne(0)) { throw "Başka bir Everything kurulumu zaten çalışıyor." }
Set-Content (Join-Path $SetupStateDir "current-stage") "başlatılıyor workspace=$Workspace model=$Model" -Encoding utf8

function Write-Step([string]$Message) {
    Write-Host "`n[Everything Kurulum] $Message" -ForegroundColor Cyan
    Set-Content (Join-Path $SetupStateDir "current-stage") $Message -Encoding utf8
}

function Assert-Windows {
    if ($env:OS -ne "Windows_NT") {
        throw "install.ps1 yalnızca Windows içindir. Linux/macOS için install.sh kullanın."
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "Bu sürüm 64 bit Windows gerektiriyor."
    }
}

function Add-ProcessPath([string]$Path) {
    if (-not $Path -or -not (Test-Path $Path)) { return }
    $parts = @($env:Path -split ';' | Where-Object { $_ })
    if ($parts -notcontains $Path) { $env:Path = "$Path;$env:Path" }
}

function Refresh-ProcessPath {
    $paths = [System.Collections.Generic.List[string]]::new()
    $known = @(
        (Join-Path $HOME ".cargo\bin"),
        (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links"),
        (Join-Path $env:LOCALAPPDATA "Programs\Ollama"),
        (Join-Path $env:ProgramFiles "nodejs")
    )
    $pythonRoot = Join-Path $env:LOCALAPPDATA "Programs\Python"
    if (Test-Path $pythonRoot) {
        foreach ($directory in @(Get-ChildItem $pythonRoot -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending)) {
            $known += $directory.FullName
            $known += (Join-Path $directory.FullName "Scripts")
        }
    }
    foreach ($entry in $known) {
        if ($entry -and (Test-Path $entry) -and -not $paths.Contains($entry)) { $paths.Add($entry) }
    }
    foreach ($scope in @("Machine", "User")) {
        $value = [Environment]::GetEnvironmentVariable("Path", $scope)
        foreach ($entry in @($value -split ';')) {
            if ($entry -and -not $paths.Contains($entry)) { $paths.Add($entry) }
        }
    }
    foreach ($entry in @($OriginalProcessPath -split ';')) {
        if ($entry -and -not $paths.Contains($entry)) { $paths.Add($entry) }
    }
    $env:Path = $paths -join ';'
}

function Invoke-WithRetry([scriptblock]$Operation, [int]$Attempts = 3) {
    $delay = 2
    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try { & $Operation; return }
        catch {
            if ($attempt -eq $Attempts) { throw }
            Write-Step "Geçici hata oluştu; yeniden deneniyor ($attempt/$Attempts): $($_.Exception.Message)"
            Start-Sleep -Seconds $delay
            $delay *= 2
        }
    }
}

function Test-IsAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-InstallerProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [switch]$Elevated,
        [switch]$WaitForProcessOnly,
        [string]$DisplayName = "Kurulum paketi"
    )
    $start = @{
        FilePath = $FilePath
        ArgumentList = $ArgumentList
        PassThru = $true
        WindowStyle = "Hidden"
    }
    if (-not $WaitForProcessOnly) { $start.Wait = $true }
    if ($Elevated -and -not (Test-IsAdministrator)) { $start.Verb = "RunAs" }
    $process = Start-Process @start
    if ($WaitForProcessOnly) { $process.WaitForExit() }
    if (@(0, 1641, 3010) -notcontains $process.ExitCode) {
        throw "$DisplayName başarısız oldu (çıkış kodu: $($process.ExitCode))."
    }
}

function Invoke-OfficialDownload([string]$Uri, [string]$Destination) {
    Invoke-WithRetry {
        Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination -MaximumRedirection 10
        if (-not (Test-Path $Destination) -or (Get-Item $Destination).Length -eq 0) {
            throw "İndirilen dosya boş: $Uri"
        }
    }
}

function Assert-ValidAuthenticodeSignature([string]$FilePath, [string]$PublisherPattern = "") {
    $signature = Get-AuthenticodeSignature -FilePath $FilePath
    if ($signature.Status -ne "Valid") {
        throw "İndirilen kurulum paketinin dijital imzası geçersiz: $FilePath ($($signature.Status))."
    }
    if ($PublisherPattern -and $signature.SignerCertificate.Subject -notmatch $PublisherPattern) {
        throw "Kurulum paketinin yayıncısı beklenenden farklı: $($signature.SignerCertificate.Subject)"
    }
}

function Install-DirectDependency([string]$WingetId) {
    $temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("everything-dependency-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $temporary | Out-Null
    try {
        switch ($WingetId) {
            "Rustlang.Rustup" {
                $installer = Join-Path $temporary "rustup-init.exe"
                Invoke-OfficialDownload "https://win.rustup.rs/x86_64" $installer
                Invoke-InstallerProcess $installer @("-y", "--profile", "minimal", "--default-toolchain", "none") -DisplayName "Rustup kurulumu"
            }
            "OpenJS.NodeJS.LTS" {
                Write-Step "winget bulunamadı; Node.js resmî kaynaktan indiriliyor"
                $releases = Invoke-RestMethod -Uri "https://nodejs.org/dist/index.json" -TimeoutSec 60
                $release = @($releases | Where-Object {
                    $_.lts -and ($_.files -contains "win-x64-msi") -and ([int]($_.version.TrimStart('v').Split('.')[0]) -ge 22)
                } | Sort-Object date -Descending | Select-Object -First 1)
                if ($release.Count -eq 0) { throw "Node.js 22+ LTS paketi bulunamadı." }
                $version = $release[0].version
                $installer = Join-Path $temporary "node-$version-x64.msi"
                Invoke-OfficialDownload "https://nodejs.org/dist/$version/node-$version-x64.msi" $installer
                Invoke-InstallerProcess "msiexec.exe" @("/i", "`"$installer`"", "/qn", "/norestart") -Elevated -DisplayName "Node.js kurulumu"
            }
            "Python.Python.3.12" {
                $installer = Join-Path $temporary "python-3.12.10-amd64.exe"
                Invoke-OfficialDownload "https://www.python.org/ftp/python/3.12.10/python-3.12.10-amd64.exe" $installer
                Invoke-InstallerProcess $installer @("/quiet", "InstallAllUsers=0", "PrependPath=1", "Include_test=0", "Include_launcher=1", "InstallLauncherAllUsers=0") -DisplayName "Python kurulumu"
            }
            "Ollama.Ollama" {
                $installer = Join-Path $temporary "OllamaSetup.exe"
                Invoke-OfficialDownload "https://ollama.com/download/OllamaSetup.exe" $installer
                Assert-ValidAuthenticodeSignature $installer "(^|, )O=Ollama Inc\.(,|$)"
                Invoke-InstallerProcess $installer @("/VERYSILENT", "/NORESTART", "/SUPPRESSMSGBOXES") -WaitForProcessOnly -DisplayName "Ollama kurulumu"
            }
            "Microsoft.VisualStudio.2022.BuildTools" {
                $installer = Join-Path $temporary "vs_BuildTools.exe"
                Invoke-OfficialDownload "https://aka.ms/vs/17/release/vs_BuildTools.exe" $installer
                Invoke-InstallerProcess $installer @(
                    "--quiet", "--wait", "--norestart", "--nocache",
                    "--add", "Microsoft.VisualStudio.Workload.VCTools", "--includeRecommended"
                ) -Elevated -DisplayName "Visual Studio C++ Build Tools kurulumu"
            }
            default { throw "$WingetId için doğrudan kurulum yöntemi tanımlı değil." }
        }
    } finally {
        Remove-Item $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Install-DependencyPackage([string]$DisplayName, [string]$WingetId, [switch]$Upgrade) {
    if (-not $InstallDeps) {
        throw "$DisplayName eksik veya eski. setup.ps1 ile yeniden çalıştırın ya da bağımlılığı elle kurun."
    }
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if ($winget) {
        Write-Step "$DisplayName winget ile kuruluyor"
        $verb = if ($Upgrade) { "upgrade" } else { "install" }
        if ($WingetId -eq "Microsoft.VisualStudio.2022.BuildTools") {
            & $winget.Source $verb --id $WingetId --exact --silent --accept-package-agreements --accept-source-agreements --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
        } else {
            & $winget.Source $verb --id $WingetId --exact --silent --accept-package-agreements --accept-source-agreements
        }
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "winget işlemi başarısız oldu; resmî doğrudan kurulum deneniyor."
            Install-DirectDependency $WingetId
        }
    } else {
        Write-Step "winget bulunamadı; $DisplayName resmî kaynaktan kuruluyor"
        Install-DirectDependency $WingetId
    }
    Refresh-ProcessPath
}

function Require-Command([string]$Name, [string]$WingetId, [string]$DisplayName = $Name) {
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    Install-DependencyPackage $DisplayName $WingetId
    Refresh-ProcessPath
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "$DisplayName kuruldu ancak bu PowerShell oturumunda bulunamadı. Günlükteki PATH bilgisini kontrol edin."
    }
    return $command.Source
}

function Find-PythonExecutable {
    Refresh-ProcessPath
    $candidates = [System.Collections.Generic.List[string]]::new()
    foreach ($name in @("python.exe", "python")) {
        foreach ($command in @(Get-Command $name -All -ErrorAction SilentlyContinue)) {
            if ($command.Source -and -not $candidates.Contains($command.Source)) { $candidates.Add($command.Source) }
        }
    }
    $pythonRoot = Join-Path $env:LOCALAPPDATA "Programs\Python"
    if (Test-Path $pythonRoot) {
        foreach ($path in @(Get-ChildItem $pythonRoot -Filter python.exe -Recurse -ErrorAction SilentlyContinue | Sort-Object FullName -Descending)) {
            if (-not $candidates.Contains($path.FullName)) { $candidates.Add($path.FullName) }
        }
    }
    $launcher = Get-Command py.exe -ErrorAction SilentlyContinue
    if ($launcher) {
        foreach ($selector in @("-3.13", "-3.12", "-3.11")) {
            try {
                $resolvedOutput = & $launcher.Source $selector -c "import sys; print(sys.executable)" 2>$null
                $resolvedExitCode = $LASTEXITCODE
                $resolved = if ($resolvedOutput) { (@($resolvedOutput)[0]).ToString().Trim() } else { "" }
                if ($resolvedExitCode -eq 0 -and $resolved -and -not $candidates.Contains($resolved)) { $candidates.Add($resolved) }
            } catch {}
        }
    }
    $valid = @()
    foreach ($candidate in $candidates) {
        if ($candidate -match '\\WindowsApps\\') { continue }
        try {
            $versionOutput = & $candidate -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}')" 2>$null
            $versionExitCode = $LASTEXITCODE
            $version = if ($versionOutput) { (@($versionOutput)[0]).ToString().Trim() } else { "" }
            if ($versionExitCode -eq 0 -and $version -match '^(\d+)\.(\d+)\.(\d+)$') {
                $preferred = 0
                if ($pythonRoot -and $candidate.StartsWith($pythonRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
                    $preferred = 1
                }
                $valid += [pscustomobject]@{
                    Path = $candidate
                    Preferred = $preferred
                    Major = [int]$Matches[1]
                    Minor = [int]$Matches[2]
                    Patch = [int]$Matches[3]
                }
            }
        } catch {}
    }
    $best = $valid | Sort-Object -Property Preferred, Major, Minor, Patch -Descending | Select-Object -First 1
    if ($best) { return $best.Path }
    return $null
}

function Ensure-Python {
    $script:PythonExe = Find-PythonExecutable
    if (-not $script:PythonExe) {
        Install-DependencyPackage "Python 3.12" "Python.Python.3.12"
        $script:PythonExe = Find-PythonExecutable
    }
    if (-not $script:PythonExe) { throw "Python 3.11+ kurulamadı veya çalıştırılamadı." }
    $version = (& $script:PythonExe -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')").Trim()
    $parts = $version.Split('.')
    if ([int]$parts[0] -lt 3 -or ([int]$parts[0] -eq 3 -and [int]$parts[1] -lt 11)) {
        if ($InstallDeps) {
            Install-DependencyPackage "Python 3.12" "Python.Python.3.12"
            $script:PythonExe = Find-PythonExecutable
            $version = (& $script:PythonExe -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')").Trim()
            $parts = $version.Split('.')
        }
        if ([int]$parts[0] -lt 3 -or ([int]$parts[0] -eq 3 -and [int]$parts[1] -lt 11)) {
            throw "Python 3.11+ gerekiyor; bulunan sürüm: $version ($script:PythonExe)."
        }
    }
}

function Get-VisualStudioInstallPath {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) { return $null }
    $path = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1)
    if ($path) { return $path.Trim() }
    return $null
}

function Import-VisualStudioEnvironment([string]$InstallPath) {
    $vsDevCmd = Join-Path $InstallPath "Common7\Tools\VsDevCmd.bat"
    if (-not (Test-Path $vsDevCmd)) { throw "VsDevCmd.bat bulunamadı: $vsDevCmd" }
    $commandLine = "`"$vsDevCmd`" -no_logo -arch=x64 -host_arch=x64 && set"
    foreach ($line in @(& $env:ComSpec /d /s /c $commandLine)) {
        if ($line -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
    }
}

function Ensure-VisualCppBuildTools {
    $installPath = Get-VisualStudioInstallPath
    if (-not $installPath) {
        Install-DependencyPackage "Visual Studio 2022 C++ Build Tools ve Windows SDK" "Microsoft.VisualStudio.2022.BuildTools"
        $installPath = Get-VisualStudioInstallPath
    }
    if (-not $installPath) {
        throw "Visual Studio C++ Build Tools kurulduktan sonra bulunamadı. Windows'u yeniden başlatıp setup.ps1 dosyasını tekrar çalıştırmanız gerekebilir."
    }
    Import-VisualStudioEnvironment $installPath
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue) -or -not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
        throw "MSVC C/C++ derleyicisi etkinleştirilemedi (cl.exe/link.exe bulunamadı)."
    }
    Write-Step "Visual C++ derleme ortamı hazır"
}

function Normalize-ComparablePath([string]$PathValue) {
    if (-not $PathValue) { return "" }
    $fullPath = [System.IO.Path]::GetFullPath($PathValue)
    if ($fullPath.StartsWith("\\?\UNC\", [System.StringComparison]::OrdinalIgnoreCase)) {
        return "\" + $fullPath.Substring(7)
    }
    if ($fullPath.StartsWith("\\?\", [System.StringComparison]::OrdinalIgnoreCase)) {
        return $fullPath.Substring(4)
    }
    return $fullPath
}

function Test-RustToolchainReady([string]$Toolchain) {
    $checks = @(
        @("rustc", "--version"),
        @("cargo", "--version"),
        @("rustfmt", "--version"),
        @("clippy-driver", "--version")
    )
    foreach ($commandArgs in $checks) {
        try {
            $output = & rustup run $Toolchain @commandArgs 2>$null
            $exitCode = $LASTEXITCODE
            if ($exitCode -ne 0 -or -not $output) { return $false }
        } catch {
            return $false
        }
    }
    return $true
}

function Wait-EverythingHealth([string]$Url, [int]$TimeoutSeconds = 90, [string]$ExpectedWorkspace = "") {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $info = Invoke-RestMethod -Uri "$Url/v1/info" -TimeoutSec 1
            $workspaceMatches = -not $ExpectedWorkspace -or (Normalize-ComparablePath $info.workspace) -eq (Normalize-ComparablePath $ExpectedWorkspace)
            if ($info.service -eq "everythingd" -and $workspaceMatches) { return }
        } catch { Start-Sleep -Milliseconds 250 }
    }
    throw "Everything servisi $Url adresinde sağlıklı duruma geçmedi."
}

function Test-PortAvailable([int]$Port) {
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
        return $true
    } catch {
        return $false
    } finally {
        try { $listener.Stop() } catch {}
    }
}

function Get-FreeLoopbackPort([int]$Excluded = 0) {
    for ($attempt = 0; $attempt -lt 8; $attempt++) {
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
        try {
            $listener.Start()
            $candidate = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
        } finally {
            try { $listener.Stop() } catch {}
        }
        if ($candidate -ne $Excluded) { return $candidate }
    }
    throw "Boş bir yerel bağlantı portu ayrılamadı."
}

function Save-ServicePorts {
    $temporary = "$PortStateFile.tmp-$PID"
    $content = @(
        "SERVICE_PORT=$ServicePort",
        "OAUTH_PORT=$OAuthPort",
        "WORKSPACE=$Workspace"
    ) -join "`n"
    [System.IO.File]::WriteAllText($temporary, "$content`n", [System.Text.UTF8Encoding]::new($false))
    Move-Item $temporary $PortStateFile -Force
}

function Prepare-ServicePorts {
    if ($NoService) { return }
    Stop-EverythingBackgroundService
    Start-Sleep -Milliseconds 300
    if (-not (Test-PortAvailable $ServicePort)) {
        if ($ServicePortExplicit) { throw "$ServicePort portu kullanımda; EVERYTHING_SERVICE_PORT için boş bir port seçin." }
        $previous = $ServicePort
        $script:ServicePort = Get-FreeLoopbackPort $OAuthPort
        Write-Step "$previous servis portu doluydu; $ServicePort seçildi ve kaydedildi"
    }
    if (-not (Test-PortAvailable $OAuthPort)) {
        if ($OAuthPortExplicit) { throw "$OAuthPort OAuth geri dönüş portu kullanımda; EVERYTHING_OAUTH_PORT için boş bir port seçin." }
        $previous = $OAuthPort
        $script:OAuthPort = Get-FreeLoopbackPort $ServicePort
        Write-Step "$previous OAuth portu doluydu; $OAuthPort seçildi. Sağlayıcılara http://127.0.0.1:$OAuthPort/v1/connectors/oauth/callback adresini kaydedin"
    }
    if ($ServicePort -eq $OAuthPort) { throw "Servis ve OAuth portları aynı değere çözümlendi." }
    Save-ServicePorts
}

function Stop-EverythingBackgroundService {
    $taskName = "Everything Runtime"
    try { Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue } catch {}
    $daemon = Join-Path $InstallDir "bin\everythingd.exe"
    Get-CimInstance Win32_Process -Filter "Name='everythingd.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.ExecutablePath -eq $daemon } |
        ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate -ErrorAction SilentlyContinue | Out-Null }
    Start-Sleep -Milliseconds 300
}

function Test-OllamaReady {
    if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) { return $false }
    try {
        & ollama list *> $null
        return $LASTEXITCODE -eq 0
    } catch {
        return $false
    }
}

function Ensure-OllamaRunning {
    if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) { return $false }
    if (Test-OllamaReady) { return $true }
    Write-Step "Ollama başlatılıyor"
    Start-Process -FilePath (Get-Command ollama).Source -ArgumentList @("serve") -WindowStyle Hidden | Out-Null
    $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        if (Test-OllamaReady) { return $true }
        Start-Sleep -Seconds 1
    }
    return $false
}

function Get-ModelRequiredMiB([string]$Name) {
    $normalized = $Name.ToLowerInvariant()
    if ($normalized -match '32b') { return 28000 }
    if ($normalized -match '14b') { return 14000 }
    if ($normalized -match '9b') { return 9000 }
    if ($normalized -match '(7b|8b)') { return 7500 }
    if ($normalized -match '(3b|4b)') { return 4200 }
    if ($normalized -match '(1\.5b|2b)') { return 2600 }
    if ($normalized -match '(0\.5b|1b)') { return 1600 }
    return 10000
}

function Assert-ModelCapacity([string]$Name) {
    $probe = if ($env:OLLAMA_MODELS) { $env:OLLAMA_MODELS } else { Join-Path $HOME ".ollama" }
    $rootPath = [System.IO.Path]::GetPathRoot([System.IO.Path]::GetFullPath($probe))
    $drive = [System.IO.DriveInfo]::new($rootPath)
    $availableMiB = [math]::Floor($drive.AvailableFreeSpace / 1MB)
    $requiredMiB = Get-ModelRequiredMiB $Name
    if ($availableMiB -lt $requiredMiB) {
        throw "$Name modeli yaklaşık $requiredMiB MiB boş alan istiyor; $rootPath üzerinde yalnızca $availableMiB MiB var."
    }
}

function Invoke-LiveModelSmoke {
    if ($NoVerify -or -not $PullModel) { return }
    Write-Step "Yerel model canlı hazırlık testi çalıştırılıyor"
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    $process = Start-Process -FilePath (Join-Path $InstallDir "bin\everythingd.exe") -ArgumentList @(
        "--workspace", $Workspace,
        "--listen", "127.0.0.1:$port",
        "--oauth-listen", "127.0.0.1:0"
    ) -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $InstallDir "model-smoke.log") -RedirectStandardError (Join-Path $InstallDir "model-smoke-error.log")
    try {
        Wait-EverythingHealth "http://127.0.0.1:$port" 30 $Workspace
        & $PythonExe (Join-Path $Root "scripts\smoke_ollama.py") --base-url "http://127.0.0.1:$port" --model $Model --mode Fast
        if ($LASTEXITCODE -ne 0) { throw "Canlı Everything/Ollama hazırlık testi başarısız oldu." }
    } finally {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
}

function Start-ResearchSidecar {
    $script:ResearchSidecarStatus = "fallback"
    if ($env:EVERYTHING_RESEARCH_SIDECAR -eq "off") {
        $script:ResearchSidecarStatus = "disabled"
        Write-Step "Yerel SearXNG yardımcısı kapalı; anahtarsız araştırma sağlayıcıları kullanılabilir"
        return
    }
    $helper = Join-Path $InstallDir "scripts\research_sidecar.ps1"
    if (-not (Test-Path $helper)) {
        Write-Step "Yerel SearXNG yardımcısı bulunamadı; anahtarsız araştırma sağlayıcıları kullanılabilir"
        return
    }
    Write-Step "İsteğe bağlı yerel SearXNG araştırma yardımcısı başlatılıyor"
    try { & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $helper start } catch { Write-Warning $_.Exception.Message }
    $port = if ($env:EVERYTHING_SEARXNG_PORT) { $env:EVERYTHING_SEARXNG_PORT } else { "8888" }
    try {
        Invoke-RestMethod -Uri "http://127.0.0.1:$port/search?q=everything&format=json" -TimeoutSec 4 | Out-Null
        $script:ResearchSidecarStatus = "ready"
    } catch {
        $script:ResearchSidecarStatus = "fallback"
    }
}

function Invoke-RuntimeDoctor {
    Write-Step "Tam yerel çalışma zamanı doktoru çalıştırılıyor"
    $reportPath = Join-Path $InstallDir "runtime-doctor.json"
    $temporary = "$reportPath.tmp-$PID"
    & (Join-Path $InstallDir "bin\everything-cli.exe") --workspace $Workspace --json doctor | Set-Content $temporary -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw "Çalışma zamanı doktoru tamamlanamadı." }
    $report = Get-Content -Raw $temporary | ConvertFrom-Json
    $required = @('model', 'graph', 'state-store', 'memory', 'skills', 'connectors', 'scheduler', 'tool-sandbox', 'data-directory', 'research')
    $ids = @($report.checks | ForEach-Object { $_.check_id })
    $missing = @($required | Where-Object { $_ -notin $ids })
    if ($missing.Count -gt 0) { throw "Çalışma zamanı doktorunda zorunlu kontroller eksik: $($missing -join ', ')" }
    $failed = @($report.checks | Where-Object { $_.status -eq 'failed' })
    if ($failed.Count -gt 0) {
        $summary = @($failed | ForEach-Object { "$($_.check_id): $($_.detail)" }) -join '; '
        throw "Çalışma zamanı doktoru başarısız bileşenler bildirdi: $summary"
    }
    if ($PullModel) {
        $modelCheck = $report.checks | Where-Object { $_.check_id -eq 'model' } | Select-Object -First 1
        if (-not $modelCheck -or $modelCheck.status -eq 'failed') {
            throw "Yapılandırılmış yerel model hazır değil: $($modelCheck.detail)"
        }
    }
    foreach ($check in @($report.checks | Where-Object { $_.status -eq 'degraded' })) {
        Write-Warning "$($check.label): $($check.detail)"
        if ($check.remediation) { Write-Warning "Remediation: $($check.remediation)" }
    }
    Move-Item $temporary $reportPath -Force
}

function Write-InstallManifest {
    $manifestPath = Join-Path $InstallDir "install-manifest.json"
    $stateManifestPath = Join-Path $SetupStateDir "last-success.json"
    $revision = "unknown"
    if (Get-Command git -ErrorAction SilentlyContinue) {
        try { $revision = (& git -C $Root rev-parse --verify HEAD 2>$null).Trim() } catch {}
    }
    function Get-ToolVersion([string]$Name) {
        if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) { return $null }
        try { return ((& $Name --version 2>$null | Select-Object -First 1) -as [string]).Trim() } catch { return $null }
    }
    $doctorPath = Join-Path $InstallDir "runtime-doctor.json"
    $doctor = if (Test-Path $doctorPath) { Get-Content -Raw $doctorPath | ConvertFrom-Json } else { $null }
    $manifest = [ordered]@{
        schema_version = 3
        product = "Everything"
        version = $Version
        installed_at = [DateTime]::UtcNow.ToString("o")
        source_revision = $revision
        workspace = $Workspace
        install_dir = $InstallDir
        model = $Model
        service = [ordered]@{
            installed = -not $NoService
            base_url = "http://127.0.0.1:$ServicePort"
            oauth_callback = "http://127.0.0.1:$OAuthPort/v1/connectors/oauth/callback"
            health_check_passed = $true
        }
        verification = [ordered]@{
            full_gates = -not $NoVerify
            native_release_build = $true
            daemon_smoke = $true
            model_pulled = [bool]$PullModel
            live_model_smoke = (-not $NoVerify -and [bool]$PullModel)
            runtime_doctor = [bool]$doctor
            runtime_doctor_status = if ($doctor) { $doctor.overall_status } else { $null }
            runtime_doctor_checks = if ($doctor) { @($doctor.checks).Count } else { 0 }
        }
        capabilities = [ordered]@{
            process_sandbox = if ($doctor) { (($doctor.checks | Where-Object { $_.check_id -eq 'tool-sandbox' } | Select-Object -First 1).status -eq 'healthy') } else { $false }
            os_secret_vault = if ($doctor) { (($doctor.checks | Where-Object { $_.check_id -eq 'connectors' } | Select-Object -First 1).status -eq 'healthy') } else { $false }
            persistent_scheduler = -not $NoService
            local_research_sidecar = $ResearchSidecarStatus
        }
        toolchain = [ordered]@{
            node = Get-ToolVersion "node"
            npm = Get-ToolVersion "npm"
            cargo = Get-ToolVersion "cargo"
            rustc = Get-ToolVersion "rustc"
            ollama = Get-ToolVersion "ollama"
        }
    }
    $json = ($manifest | ConvertTo-Json -Depth 8) + "`n"
    foreach ($destination in @($manifestPath, $stateManifestPath)) {
        $temporary = "$destination.tmp-$PID"
        [System.IO.File]::WriteAllText($temporary, $json, [System.Text.UTF8Encoding]::new($false))
        Move-Item $temporary $destination -Force
    }
}

function Install-BackgroundService {
    if ($NoService) { return }
    Write-Step "Kalıcı zamanlayıcı ve bağlayıcı servisi kuruluyor"
    Stop-EverythingBackgroundService
    if (-not (Test-PortAvailable $ServicePort)) { throw "$ServicePort portunu başka bir işlem kullanıyor. EVERYTHING_SERVICE_PORT değişkenini boş bir yerel porta ayarlayın." }
    if (-not (Test-PortAvailable $OAuthPort)) { throw "$OAuthPort OAuth portunu başka bir işlem kullanıyor. EVERYTHING_OAUTH_PORT değişkenini boş bir yerel porta ayarlayın." }

    $daemon = Join-Path $InstallDir "bin\everythingd.exe"
    $serviceScript = Join-Path $BinDir "everythingd-background.ps1"
    $escapedWorkspace = $Workspace.Replace("'", "''")
    $escapedDaemon = $daemon.Replace("'", "''")
    $serviceBody = @"
`$ErrorActionPreference = 'Stop'
`$env:EVERYTHING_WORKSPACE = '$escapedWorkspace'
`$env:EVERYTHING_HOME = '$(($HOME + '\.everything').Replace("'", "''"))'
& '$escapedDaemon' --workspace '$escapedWorkspace' --listen '127.0.0.1:$ServicePort' --oauth-listen '127.0.0.1:$OAuthPort'
"@
    Set-Content $serviceScript $serviceBody -Encoding utf8

    $taskName = "Everything Runtime"
    $startupFile = Join-Path ([Environment]::GetFolderPath("Startup")) "Everything Runtime.cmd"
    $installedScheduledTask = $false
    try {
        $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$serviceScript`""
        $trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
        $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 10 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
        $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
        Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null
        Remove-Item $startupFile -Force -ErrorAction SilentlyContinue
        Start-ScheduledTask -TaskName $taskName
        $installedScheduledTask = $true
    } catch {
        Write-Warning "Zamanlanmış Görev kaydı başarısız; kullanıcı Başlangıç klasörü yedeği kullanılacak: $($_.Exception.Message)"
        $command = "@echo off`r`nstart `"`" /min powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$serviceScript`"`r`n"
        Set-Content $startupFile $command -Encoding ascii
        Start-Process -FilePath "powershell.exe" -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File", $serviceScript) -WindowStyle Hidden | Out-Null
    }
    Wait-EverythingHealth "http://127.0.0.1:$ServicePort" 30 $Workspace
    if ($installedScheduledTask) { Write-Step "Arka plan çalışma zamanı kullanıcı Zamanlanmış Görevi olarak kuruldu" }
    else { Write-Step "Arka plan çalışma zamanı kullanıcı Başlangıç klasörüyle kuruldu" }
}

function Restore-PreviousInstallation {
    if (-not $InstallSwitched -or $InstallComplete) { return }
    Stop-EverythingBackgroundService
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path $InstallBackup) {
        Move-Item $InstallBackup $InstallDir
        try { Start-ScheduledTask -TaskName "Everything Runtime" -ErrorAction SilentlyContinue } catch {}
    }
}

try {
    Assert-Windows
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    New-Item -ItemType Directory -Force -Path $Workspace | Out-Null
    Refresh-ProcessPath

    Write-Step "Windows derleme ön koşulları denetleniyor"
    Ensure-VisualCppBuildTools

    Require-Command "rustup" "Rustlang.Rustup" "Rustup" | Out-Null
    Write-Step "Rust $RustToolchain araç zinciri hazırlanıyor"
    & rustup toolchain install $RustToolchain --profile minimal --component rustfmt --component clippy
    if ($LASTEXITCODE -ne 0) {
        if (Test-RustToolchainReady $RustToolchain) {
            Write-Warning "Rust araç zinciri indirilemedi ancak $RustToolchain zaten kullanılabilir durumda; kurulum mevcut araç zinciriyle devam edecek."
        } else {
            throw "Rust $RustToolchain araç zinciri kurulamadı."
        }
    }
    $env:RUSTUP_TOOLCHAIN = $RustToolchain
    Refresh-ProcessPath
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "Rust kurulumu cargo komutunu sağlamadı." }
    $rustVersion = (& rustc --version).Trim()
    if ($LASTEXITCODE -ne 0 -or $rustVersion -notmatch "rustc $([regex]::Escape($RustToolchain))") {
        throw "Beklenen Rust sürümü $RustToolchain, bulunan: $rustVersion"
    }

    Require-Command "node" "OpenJS.NodeJS.LTS" "Node.js LTS" | Out-Null
    Require-Command "npm" "OpenJS.NodeJS.LTS" "npm" | Out-Null
    $nodeMajor = [int]((node -p "process.versions.node.split('.')[0]").Trim())
    if ($nodeMajor -lt 22) {
        Install-DependencyPackage "Node.js 22+ LTS" "OpenJS.NodeJS.LTS" -Upgrade
        Refresh-ProcessPath
        $nodeMajor = [int]((node -p "process.versions.node.split('.')[0]").Trim())
    }
    if ($nodeMajor -lt 22) { throw "Node.js 22 veya yenisi gerekiyor; bulunan ana sürüm: $nodeMajor." }
    $npmMajor = [int]((npm --version).Trim().Split('.')[0])
    if ($npmMajor -lt 10) {
        Write-Step "npm 10 veya yenisine yükseltiliyor"
        Invoke-WithRetry { npm install --global npm@10; if ($LASTEXITCODE -ne 0) { throw "npm yükseltilemedi." } }
        Refresh-ProcessPath
        $npmMajor = [int]((npm --version).Trim().Split('.')[0])
    }
    if ($npmMajor -lt 10) { throw "npm 10 veya yenisi gerekiyor; bulunan ana sürüm: $npmMajor." }

    Ensure-Python
    if ($InstallDeps -or $PullModel) {
        Require-Command "ollama" "Ollama.Ollama" "Ollama" | Out-Null
    }

    Write-Step "Tüm bağımlılıklar hazır"
    Write-Host "  $(& rustc --version)"
    Write-Host "  node $(node --version)"
    Write-Host "  npm  $(npm --version)"
    Write-Host "  python $(& $PythonExe --version 2>&1)"
    if (Get-Command ollama -ErrorAction SilentlyContinue) { Write-Host "  $(& ollama --version 2>&1 | Select-Object -First 1)" }

    if (-not $NoVerify) {
        Write-Step "Sürüm ve kurucu sözleşmeleri denetleniyor"
        & $PythonExe (Join-Path $Root "scripts\check_versions.py")
        if ($LASTEXITCODE -ne 0) { throw "Proje sürümleri birbiriyle eşleşmiyor." }
        & $PythonExe (Join-Path $Root "scripts\smoke_installers.py")
        if ($LASTEXITCODE -ne 0) { throw "Kurucu sözleşme testi başarısız oldu." }
    }

    Write-Step "Rust çalışma zamanı biçimlendiriliyor, denetleniyor, test ediliyor ve derleniyor"
    Push-Location $Root
    try {
        if (-not $NoVerify) {
            cargo fmt --all -- --check
            if ($LASTEXITCODE -ne 0) { throw "cargo fmt --check başarısız. Kaynak biçimi rustfmt standardına uymuyor." }
            cargo clippy --locked --workspace --all-targets -- -D warnings
            if ($LASTEXITCODE -ne 0) { throw "cargo clippy kalite denetimi başarısız oldu." }
            cargo test --locked --workspace --all-targets
            if ($LASTEXITCODE -ne 0) { throw "Rust testleri başarısız oldu." }
        }
        cargo build --release --locked --workspace
        if ($LASTEXITCODE -ne 0) { throw "Rust release derlemesi başarısız oldu." }
    } finally { Pop-Location }

    Write-Step "Electron masaüstü uygulaması kuruluyor ve derleniyor"
    $AppRoot = Join-Path $Root "apps\everything-app"
    Push-Location $AppRoot
    try {
        Invoke-WithRetry { npm ci; if ($LASTEXITCODE -ne 0) { throw "npm ci bağımlılık kurulumu başarısız oldu." } }
        Invoke-WithRetry { node node_modules/electron/install.js; if ($LASTEXITCODE -ne 0) { throw "Electron çalışma zamanı indirilemedi." } }
        $SourceElectron = Join-Path $AppRoot "node_modules\electron\dist\electron.exe"
        if (-not (Test-Path $SourceElectron)) { throw "Electron platform çalışma zamanı doğru kurulmadı." }
        npm run typecheck
        if ($LASTEXITCODE -ne 0) { throw "Electron TypeScript tip denetimi başarısız oldu." }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "Electron derlemesi başarısız oldu." }
        npm audit --omit=dev --audit-level=high
        if ($LASTEXITCODE -ne 0) { throw "Electron üretim bağımlılığı güvenlik denetimi başarısız oldu." }
    } finally { Pop-Location }

    if (-not $NoVerify) {
        Write-Step "Python SDK yalıtılmış ortamda test ediliyor ve paketleniyor"
        $PythonRoot = Join-Path $Root "python\everything_control"
        $Venv = Join-Path $Root ".venv-mvp"
        Push-Location $PythonRoot
        try {
            Remove-Item $Venv -Recurse -Force -ErrorAction SilentlyContinue
            & $PythonExe -m venv $Venv
            if ($LASTEXITCODE -ne 0) { throw "Python sanal ortamı oluşturulamadı." }
            $VenvPython = Join-Path $Venv "Scripts\python.exe"
            Invoke-WithRetry { & $VenvPython -m pip install --upgrade pip; if ($LASTEXITCODE -ne 0) { throw "pip güncellenemedi." } }
            Invoke-WithRetry { & $VenvPython -m pip install -e '.[dev]'; if ($LASTEXITCODE -ne 0) { throw "Python SDK bağımlılıkları kurulamadı." } }
            & $VenvPython -m pytest
            if ($LASTEXITCODE -ne 0) { throw "Python SDK testleri başarısız oldu." }
            & $VenvPython -m build
            if ($LASTEXITCODE -ne 0) { throw "Python SDK paketi oluşturulamadı." }
        } finally {
            Pop-Location
            Remove-Item $Venv -Recurse -Force -ErrorAction SilentlyContinue
        }
        Push-Location $Root
        try {
            & $PythonExe scripts/smoke_mvp.py --require-built-ui
            if ($LASTEXITCODE -ne 0) { throw "Statik MVP duman testi başarısız oldu." }
        } finally { Pop-Location }
    }

    Write-Step "Everything $InstallDir klasörüne atomik olarak kuruluyor"
    $TempInstall = "$InstallDir.tmp"
    Remove-Item $TempInstall -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $TempInstall "bin"), (Join-Path $TempInstall "app"), (Join-Path $TempInstall "deploy"), (Join-Path $TempInstall "scripts"), $BinDir, (Join-Path $HOME ".everything\skills") | Out-Null
    Copy-Item (Join-Path $Root "target\release\everythingd.exe") (Join-Path $TempInstall "bin\everythingd.exe")
    Copy-Item (Join-Path $Root "target\release\everything-cli.exe") (Join-Path $TempInstall "bin\everything-cli.exe")
    Copy-Item (Join-Path $AppRoot "out") (Join-Path $TempInstall "app\out") -Recurse
    Copy-Item (Join-Path $AppRoot "package.json") (Join-Path $TempInstall "app\package.json")
    Copy-Item (Join-Path $AppRoot "package-lock.json") (Join-Path $TempInstall "app\package-lock.json")
    Copy-Item (Join-Path $AppRoot "node_modules") (Join-Path $TempInstall "app\node_modules") -Recurse
    Copy-Item (Join-Path $Root "deploy\searxng") (Join-Path $TempInstall "deploy\searxng") -Recurse
    Copy-Item (Join-Path $Root "scripts\research_sidecar.ps1") (Join-Path $TempInstall "scripts\research_sidecar.ps1")
    Push-Location (Join-Path $TempInstall "app")
    try {
        npm prune --omit=dev
        if ($LASTEXITCODE -ne 0) { throw "npm üretim budaması başarısız oldu." }
        npm audit --omit=dev --audit-level=high
        if ($LASTEXITCODE -ne 0) { throw "Üretim bağımlılığı güvenlik denetimi başarısız oldu." }
    } finally { Pop-Location }
    $config = (Get-Content (Join-Path $Root "everything.toml") -Raw) -replace '(?m)^model_name = .*$', "model_name = `"$Model`""
    [System.IO.File]::WriteAllText((Join-Path $TempInstall "everything.toml"), $config, [System.Text.UTF8Encoding]::new($false))
    Set-Content (Join-Path $TempInstall "VERSION") $Version -Encoding ascii

    Remove-Item $InstallBackup -Recurse -Force -ErrorAction SilentlyContinue
    if (Test-Path $InstallDir) { Move-Item $InstallDir $InstallBackup }
    try {
        Move-Item $TempInstall $InstallDir
    } catch {
        Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path $InstallBackup) { Move-Item $InstallBackup $InstallDir }
        throw "Atomik kurulum geçişi başarısız oldu; önceki kurulum geri yüklendi: $($_.Exception.Message)"
    }
    $InstallSwitched = $true
    Prepare-ServicePorts

    $launcher = @"
`$ErrorActionPreference = 'Stop'
`$EngineHome = '$($InstallDir.Replace("'", "''"))'
`$Workspace = if (`$env:EVERYTHING_WORKSPACE) { `$env:EVERYTHING_WORKSPACE } else { (Get-Location).Path }
if (`$args.Count -ge 2 -and `$args[0] -eq '--workspace') {
    `$Workspace = `$args[1]
    `$args = @(`$args | Select-Object -Skip 2)
}
New-Item -ItemType Directory -Force -Path `$Workspace | Out-Null
`$Workspace = [System.IO.Path]::GetFullPath(`$Workspace)
if (-not (Test-Path (Join-Path `$Workspace 'everything.toml'))) { Copy-Item (Join-Path `$EngineHome 'everything.toml') (Join-Path `$Workspace 'everything.toml') }
`$env:EVERYTHING_WORKSPACE = `$Workspace
`$env:EVERYTHINGD_BIN = Join-Path `$EngineHome 'bin\everythingd.exe'
if (-not `$env:EVERYTHINGD_URL) { `$env:EVERYTHINGD_URL = 'http://127.0.0.1:$ServicePort' }
if (-not `$env:EVERYTHING_HOME) { `$env:EVERYTHING_HOME = Join-Path `$HOME '.everything' }
`$Electron = Join-Path `$EngineHome 'app\node_modules\electron\dist\electron.exe'
if (-not (Test-Path `$Electron)) { throw "Everything Electron runtime is missing: `$Electron" }
& `$Electron (Join-Path `$EngineHome 'app') @args
exit `$LASTEXITCODE
"@
    Set-Content (Join-Path $BinDir "everything.ps1") $launcher -Encoding utf8
    Set-Content (Join-Path $BinDir "everything.cmd") "@powershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0everything.ps1`" %*" -Encoding ascii
    Set-Content (Join-Path $BinDir "everything-cli.cmd") "@`"$InstallDir\bin\everything-cli.exe`" %*" -Encoding ascii

    New-Item -ItemType Directory -Force -Path $Workspace | Out-Null
    if (-not (Test-Path (Join-Path $Workspace "everything.toml"))) { Copy-Item (Join-Path $InstallDir "everything.toml") (Join-Path $Workspace "everything.toml") }

    Write-Step "Kurulan servis için canlı duman testi çalıştırılıyor"
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    $listener.Stop()
    $daemon = Start-Process -FilePath (Join-Path $InstallDir "bin\everythingd.exe") -ArgumentList @("--workspace", $Workspace, "--listen", "127.0.0.1:$port", "--oauth-listen", "127.0.0.1:0") -PassThru -WindowStyle Hidden
    try { Wait-EverythingHealth "http://127.0.0.1:$port" 30 $Workspace }
    finally { if (-not $daemon.HasExited) { Stop-Process -Id $daemon.Id -Force -ErrorAction SilentlyContinue } }

    if ($PullModel) {
        if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) { throw "Ollama bulunamadı." }
        if (-not (Ensure-OllamaRunning)) { throw "Ollama başlatılamadı." }
        Assert-ModelCapacity $Model
        Write-Step "Ollama modeli indiriliyor: $Model"
        Invoke-WithRetry { ollama pull $Model; if ($LASTEXITCODE -ne 0) { throw "Ollama modeli indirilemedi." } }
        Invoke-LiveModelSmoke
    } elseif (-not (Get-Command ollama -ErrorAction SilentlyContinue)) {
        Write-Step "Ollama kurulmadı; yerel model görevlerinden önce kurmanız gerekir"
    }

    Install-BackgroundService
    Start-ResearchSidecar
    Invoke-RuntimeDoctor
    Write-Step "Doğrulanmış kurulum manifestosu yazılıyor"
    Write-InstallManifest

    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $currentUserPath) { $currentUserPath = "" }
    if (($currentUserPath -split ';') -notcontains $BinDir) {
        [Environment]::SetEnvironmentVariable("Path", (($currentUserPath.TrimEnd(';') + ';' + $BinDir).Trim(';')), "User")
        Write-Step "$BinDir kullanıcı PATH değişkenine eklendi; yeni terminalde everything komutunu kullanabilirsiniz"
    }

    $InstallComplete = $true
    Remove-Item $InstallBackup -Recurse -Force -ErrorAction SilentlyContinue
    Write-Step "Everything başarıyla kuruldu"
    Set-Content (Join-Path $SetupStateDir "current-stage") "tamamlandı" -Encoding utf8
} catch {
    Restore-PreviousInstallation
    Set-Content (Join-Path $SetupStateDir "current-stage") "başarısız: $($_.Exception.Message)" -Encoding utf8
    throw
} finally {
    try { $InstallMutex.ReleaseMutex() } catch {}
    $InstallMutex.Dispose()
}

if ($ShouldLaunch) { & (Join-Path $BinDir "everything.ps1") --workspace $Workspace }
