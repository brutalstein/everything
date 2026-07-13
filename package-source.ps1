param(
    [string]$OutputPath = (Join-Path $PSScriptRoot "everything-source.zip")
)

$ErrorActionPreference = "Stop"
$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) {
    $python = Get-Command py -ErrorAction SilentlyContinue
}
if (-not $python) {
    throw "Python 3.11+ is required to package the source tree."
}

& $python.Source (Join-Path $PSScriptRoot "scripts/package_source.py") --output $OutputPath
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
