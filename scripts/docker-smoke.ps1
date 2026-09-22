param(
    [string]$Registry = $env:REGISTRY_HOST,
    [string]$Repository = $(if ($env:REGISTRY_REPOSITORY) { $env:REGISTRY_REPOSITORY } else { "conformance/smoke" }),
    [string]$Username = $env:REGISTRY_USERNAME,
    [string]$Password = $env:REGISTRY_PASSWORD,
    [string]$SourceImage = $(if ($env:REGISTRY_SOURCE_IMAGE) { $env:REGISTRY_SOURCE_IMAGE } else { "busybox:latest" })
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ([string]::IsNullOrWhiteSpace($Registry) -or [string]::IsNullOrWhiteSpace($Username) -or [string]::IsNullOrWhiteSpace($Password)) {
    throw "Set REGISTRY_HOST, REGISTRY_USERNAME, and REGISTRY_PASSWORD for a disposable target."
}
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "Docker CLI is required for the compatibility smoke test."
}

$loggedIn = $false
try {
    $Password | docker login $Registry --username $Username --password-stdin
    if ($LASTEXITCODE -ne 0) {
        throw "docker login failed for $Registry"
    }
    $loggedIn = $true

    $localTag = "$Registry/$Repository`:smoke"
    docker pull $SourceImage
    if ($LASTEXITCODE -ne 0) {
        throw "docker pull failed for $SourceImage"
    }
    docker tag $SourceImage $localTag
    if ($LASTEXITCODE -ne 0) {
        throw "docker tag failed for $localTag"
    }
    docker push $localTag
    if ($LASTEXITCODE -ne 0) {
        throw "docker push failed for $localTag"
    }
    docker image rm $localTag 2>$null
    docker pull $localTag
    if ($LASTEXITCODE -ne 0) {
        throw "docker pull failed for the pushed tag $localTag"
    }
    docker image inspect $localTag --format '{{index .RepoDigests 0}}'
    if ($LASTEXITCODE -ne 0) {
        throw "docker image inspect failed for $localTag"
    }
} finally {
    if ($loggedIn) {
        docker logout $Registry | Out-Null
    }
}
