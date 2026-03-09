[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir = (Join-Path $HOME ".nex\bin"),
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DefaultReleaseBaseUrl = "https://github.com/NexumCorpus/Nexum-Graph/releases"
$DefaultApiUrl = "https://api.github.com/repos/NexumCorpus/Nexum-Graph/releases/latest"

function Fail([string]$Message) {
    throw $Message
}

function Normalize-Version([string]$Value) {
    if ($Value -match '^v?(\d+\.\d+\.\d+)$') {
        return $Matches[1]
    }
    Fail "Expected version like 0.1.0 or v0.1.0, got: $Value"
}

function Resolve-Version([string]$Candidate, [string]$ReleaseBaseUrl, [string]$ApiUrl) {
    if ($Candidate) {
        return Normalize-Version $Candidate
    }

    if ($ReleaseBaseUrl -ne $DefaultReleaseBaseUrl -and -not $env:NEXUM_GRAPH_API_URL) {
        Fail "Version is required when NEXUM_GRAPH_RELEASE_BASE_URL is overridden."
    }

    $response = Invoke-RestMethod `
        -Uri $ApiUrl `
        -Headers @{
            "User-Agent" = "nexum-graph-installer"
            "Accept" = "application/vnd.github+json"
        }

    if (-not $response.tag_name) {
        Fail "Could not determine latest release from $ApiUrl"
    }

    return Normalize-Version $response.tag_name
}

function Resolve-Target() {
    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($architecture) {
        ([System.Runtime.InteropServices.Architecture]::X64) { return "x86_64-pc-windows-msvc" }
        default { Fail "Unsupported Windows architecture: $architecture" }
    }
}

function Download-File([string]$Url, [string]$Destination) {
    Invoke-WebRequest `
        -Uri $Url `
        -OutFile $Destination `
        -Headers @{
            "User-Agent" = "nexum-graph-installer"
            "Accept" = "application/octet-stream"
        }
}

function Get-ExpectedChecksum([string]$ChecksumFile, [string]$AssetName) {
    foreach ($line in Get-Content -LiteralPath $ChecksumFile) {
        if ($line -match '^(?<hash>[0-9a-f]{64})\s{2}(?<name>.+)$' -and $Matches.name -eq $AssetName) {
            return $Matches.hash.ToLowerInvariant()
        }
    }
    Fail "Missing checksum entry for $AssetName"
}

function Ensure-OverwriteAllowed([string]$Path, [switch]$AllowOverwrite) {
    if ((Test-Path -LiteralPath $Path) -and -not $AllowOverwrite) {
        Fail "Refusing to overwrite $Path without -Force"
    }
}

$ReleaseBaseUrl = if ($env:NEXUM_GRAPH_RELEASE_BASE_URL) {
    $env:NEXUM_GRAPH_RELEASE_BASE_URL.TrimEnd("/")
} else {
    $DefaultReleaseBaseUrl
}

$ApiUrl = if ($env:NEXUM_GRAPH_API_URL) {
    $env:NEXUM_GRAPH_API_URL
} else {
    $DefaultApiUrl
}

$ResolvedVersion = Resolve-Version -Candidate $Version -ReleaseBaseUrl $ReleaseBaseUrl -ApiUrl $ApiUrl
$Tag = "v$ResolvedVersion"
$Target = Resolve-Target
$ArchiveName = "nexum-graph-v$ResolvedVersion-$Target.zip"
$ArchiveUrl = "$ReleaseBaseUrl/download/$Tag/$ArchiveName"
$ChecksumUrl = "$ReleaseBaseUrl/download/$Tag/SHA256SUMS.txt"

$TempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("nexum-graph-install-" + [Guid]::NewGuid().ToString("N"))
$ArchivePath = Join-Path $TempRoot $ArchiveName
$ChecksumPath = Join-Path $TempRoot "SHA256SUMS.txt"
$ExtractRoot = Join-Path $TempRoot "extract"

New-Item -ItemType Directory -Path $TempRoot | Out-Null
New-Item -ItemType Directory -Path $ExtractRoot | Out-Null

try {
    Download-File -Url $ChecksumUrl -Destination $ChecksumPath
    Download-File -Url $ArchiveUrl -Destination $ArchivePath

    $ExpectedChecksum = Get-ExpectedChecksum -ChecksumFile $ChecksumPath -AssetName $ArchiveName
    $ActualChecksum = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($ExpectedChecksum -ne $ActualChecksum) {
        Fail "Checksum mismatch for $ArchiveName"
    }

    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $ExtractRoot -Force
    $BundleRoot = Get-ChildItem -LiteralPath $ExtractRoot -Directory | Select-Object -First 1
    if (-not $BundleRoot) {
        Fail "Release archive did not contain a bundle directory."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    foreach ($Binary in @("nex.exe", "nex-lsp.exe")) {
        $SourcePath = Join-Path $BundleRoot.FullName $Binary
        if (-not (Test-Path -LiteralPath $SourcePath)) {
            Fail "Missing binary in archive: $Binary"
        }

        $DestinationPath = Join-Path $InstallDir $Binary
        Ensure-OverwriteAllowed -Path $DestinationPath -AllowOverwrite:$Force
        Copy-Item -LiteralPath $SourcePath -Destination $DestinationPath -Force
    }

    Write-Host "Installed nex.exe and nex-lsp.exe to $InstallDir"

    $PathEntries = @()
    foreach ($Scope in @("Process", "User")) {
        $Value = [Environment]::GetEnvironmentVariable("Path", $Scope)
        if ($Value) {
            $PathEntries += $Value.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)
        }
    }
    if ($PathEntries -notcontains $InstallDir) {
        Write-Host "Add this directory to PATH:"
        Write-Host "  $InstallDir"
        Write-Host ""
        Write-Host "PowerShell:"
        Write-Host "  [Environment]::SetEnvironmentVariable(""Path"", ""$InstallDir;"" + [Environment]::GetEnvironmentVariable(""Path"", ""User""), ""User"")"
    }
}
finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
