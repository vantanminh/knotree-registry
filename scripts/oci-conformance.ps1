param(
    [string]$Registry = $env:OCI_REGISTRY,
    [string]$Username = $env:OCI_USERNAME,
    [string]$Password = $env:OCI_PASSWORD,
    [string]$Tls = $(if ($env:OCI_TLS) { $env:OCI_TLS } else { "disabled" })
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ([string]::IsNullOrWhiteSpace($Registry) -or [string]::IsNullOrWhiteSpace($Username) -or [string]::IsNullOrWhiteSpace($Password)) {
    throw "Set OCI_REGISTRY, OCI_USERNAME, and OCI_PASSWORD for a disposable registry target."
}
if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
    throw "Go 1.24+ is required by the OCI Distribution Spec conformance suite."
}

$source = Join-Path ([IO.Path]::GetTempPath()) "knotree-distribution-spec"
if (-not (Test-Path (Join-Path $source ".git"))) {
    git clone --depth 1 https://github.com/opencontainers/distribution-spec.git $source
}
Push-Location (Join-Path $source "conformance")
try {
    $env:OCI_REGISTRY = $Registry
    $env:OCI_USERNAME = $Username
    $env:OCI_PASSWORD = $Password
    $env:OCI_TLS = $Tls
    $env:OCI_VERSION = "1.1"
    $env:OCI_REPO1 = "conformance/repo1"
    $env:OCI_REPO2 = "conformance/repo2"
    $env:OCI_RESULTS_DIR = (Join-Path (Get-Location).Path "results")
    go run -buildvcs=true .
} finally {
    Pop-Location
}
