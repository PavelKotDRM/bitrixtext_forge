[CmdletBinding()]
param(
    [ValidateSet("linux/x86_64", "linux/aarch64", "linux/amd64", "linux/arm64")]
    [string[]]$Platform = @("linux/x86_64", "linux/aarch64"),

    [string]$OutputDirectory = "dist",

    [switch]$Clean,

    [switch]$SkipWindows
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$dockerfile = Join-Path $projectRoot "Dockerfile"
$cargoManifest = Join-Path $projectRoot "Cargo.toml"
$outputRoot = Join-Path $projectRoot $OutputDirectory

function Get-LinuxTarget {
    param([string]$Value)

    switch ($Value) {
        "linux/x86_64" { return [pscustomobject]@{ DockerPlatform = "linux/amd64"; DockerArchitecture = "amd64"; Architecture = "x86_64" } }
        "linux/amd64"  { return [pscustomobject]@{ DockerPlatform = "linux/amd64"; DockerArchitecture = "amd64"; Architecture = "x86_64" } }
        "linux/aarch64" { return [pscustomobject]@{ DockerPlatform = "linux/arm64"; DockerArchitecture = "arm64"; Architecture = "aarch64" } }
        "linux/arm64"  { return [pscustomobject]@{ DockerPlatform = "linux/arm64"; DockerArchitecture = "arm64"; Architecture = "aarch64" } }
        default { throw "Unsupported Linux platform: $Value" }
    }
}

if ($Clean -and (Test-Path $outputRoot)) {
    Remove-Item -Recurse -Force $outputRoot
}

foreach ($targetPlatform in $Platform) {
    $target = Get-LinuxTarget $targetPlatform
    $artifactDirectory = Join-Path $outputRoot "linux-$($target.Architecture)"
    New-Item -ItemType Directory -Force -Path $artifactDirectory | Out-Null

    Write-Host "Building BitrixText Forge for $($target.DockerPlatform) ($($target.Architecture))..."
    docker buildx build `
        --platform $target.DockerPlatform `
        --file $dockerfile `
        --output "type=local,dest=$artifactDirectory" `
        --provenance=false `
        $projectRoot

    if ($LASTEXITCODE -ne 0) {
        throw "Docker Buildx failed for $($target.DockerPlatform)."
    }

    $dockerArtifact = Join-Path $artifactDirectory "bitrixtext_forge-linux-$($target.DockerArchitecture)"
    $canonicalArtifact = Join-Path $artifactDirectory "bitrixtext_forge-linux-$($target.Architecture)"
    if (-not (Test-Path $dockerArtifact)) {
        throw "Docker build completed, but the artifact was not found: $dockerArtifact"
    }
    if ($dockerArtifact -ne $canonicalArtifact) {
        if (Test-Path $canonicalArtifact) {
            Remove-Item -Force $canonicalArtifact
        }
        Rename-Item -Path $dockerArtifact -NewName (Split-Path -Leaf $canonicalArtifact)
    }
}

if (-not $SkipWindows) {
    $windowsDirectory = Join-Path $outputRoot "windows-x86_64"
    New-Item -ItemType Directory -Force -Path $windowsDirectory | Out-Null

    Write-Host "Building BitrixText Forge for Windows (x86_64)..."
    & cargo build --release --locked --manifest-path $cargoManifest
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed for Windows x86_64."
    }

    $windowsSource = Join-Path $projectRoot "target\release\bitrixtext_forge.exe"
    $windowsArtifact = Join-Path $windowsDirectory "bitrixtext_forge-windows-x86_64.exe"
    if (-not (Test-Path $windowsSource)) {
        throw "Windows build completed, but the artifact was not found: $windowsSource"
    }
    Copy-Item -Path $windowsSource -Destination $windowsArtifact -Force
}

Write-Host "Artifacts are available in $outputRoot"
