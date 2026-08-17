[CmdletBinding()]
param(
    [ValidateSet("linux/amd64", "linux/arm64")]
    [string[]]$Platform = @("linux/amd64", "linux/arm64"),

    [string]$OutputDirectory = "dist",

    [switch]$Clean
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$dockerfile = Join-Path $projectRoot "Dockerfile"
$outputRoot = Join-Path $projectRoot $OutputDirectory

if ($Clean -and (Test-Path $outputRoot)) {
    Remove-Item -Recurse -Force $outputRoot
}

foreach ($targetPlatform in $Platform) {
    $architecture = $targetPlatform.Split("/")[1]
    $artifactDirectory = Join-Path $outputRoot "linux-$architecture"
    New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null

    Write-Host "Building BitrixText Forge for $targetPlatform..."
    docker buildx build `
        --platform $targetPlatform `
        --file $dockerfile `
        --output "type=local,dest=$artifactDirectory" `
        --provenance=false `
        $projectRoot

    if ($LASTEXITCODE -ne 0) {
        throw "Docker Buildx failed for $targetPlatform."
    }
}

Write-Host "Artifacts are available in $outputRoot"