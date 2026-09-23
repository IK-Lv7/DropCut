$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ffmpegArchiveName = "ffmpeg-n8.1.3-win64-gpl-8.1.zip"
$ffmpegUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-22-13-18/$ffmpegArchiveName"
$ffmpegSha256 = "ed885833e406f0f7a1ea0304dd170779117799b2fba8096abc3e2e4dd679cc20"
$whisperTag = "v1.9.4"
$whisperRepository = "https://github.com/ggml-org/whisper.cpp.git"
$llamaArchiveName = "llama-b11140-bin-win-cpu-x64.zip"
$llamaUrl = "https://github.com/ggml-org/llama.cpp/releases/download/b11140/$llamaArchiveName"
$llamaSha256 = "43de4c111a7c764fd4b4df64f96e56c73d8cff2b0ffb95ec49036989a8e7d5bd"
$llmModelName = "qwen2.5-1.5b-instruct-q4_k_m.gguf"
$llmModelUrl = "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/$llmModelName"
$llmModelSha256 = "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$toolsDirectory = Join-Path $repositoryRoot "src-tauri/resources/tools"
$llmDirectory = Join-Path $repositoryRoot "src-tauri/resources/llm"
$licenseDirectory = Join-Path $repositoryRoot "src-tauri/resources/licenses/generated"
$temporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$workDirectory = Join-Path $temporaryRoot ("dropcut-tools-" + [System.Guid]::NewGuid().ToString("N"))

function Copy-SingleMatch {
    param(
        [Parameter(Mandatory = $true)][string]$SearchRoot,
        [Parameter(Mandatory = $true)][string]$FileName,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $matches = @(Get-ChildItem -Path $SearchRoot -Filter $FileName -File -Recurse)
    if ($matches.Count -ne 1) {
        throw "Expected exactly one $FileName under $SearchRoot, found $($matches.Count)."
    }
    Copy-Item -LiteralPath $matches[0].FullName -Destination $Destination -Force
}

try {
    New-Item -ItemType Directory -Path $workDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $toolsDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null

    $ffmpegArchive = Join-Path $workDirectory $ffmpegArchiveName
    Invoke-WebRequest -Uri $ffmpegUrl -OutFile $ffmpegArchive
    $actualHash = (Get-FileHash -LiteralPath $ffmpegArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $ffmpegSha256) {
        throw "FFmpeg archive checksum mismatch. Expected $ffmpegSha256, received $actualHash."
    }

    $ffmpegDirectory = Join-Path $workDirectory "ffmpeg"
    Expand-Archive -LiteralPath $ffmpegArchive -DestinationPath $ffmpegDirectory
    Copy-SingleMatch -SearchRoot $ffmpegDirectory -FileName "ffmpeg.exe" -Destination $toolsDirectory
    Copy-SingleMatch -SearchRoot $ffmpegDirectory -FileName "ffprobe.exe" -Destination $toolsDirectory

    $ffmpegLicense = @(Get-ChildItem -Path $ffmpegDirectory -Filter "LICENSE.txt" -File -Recurse)
    if ($ffmpegLicense.Count -ne 1) {
        throw "Expected exactly one FFmpeg LICENSE.txt, found $($ffmpegLicense.Count)."
    }
    Copy-Item -LiteralPath $ffmpegLicense[0].FullName -Destination (Join-Path $licenseDirectory "FFmpeg-LICENSE.txt") -Force

    $whisperSource = Join-Path $workDirectory "whisper.cpp"
    $whisperBuild = Join-Path $workDirectory "whisper-build"
    git clone --depth 1 --branch $whisperTag $whisperRepository $whisperSource
    if ($LASTEXITCODE -ne 0) { throw "Failed to clone whisper.cpp." }

    cmake -S $whisperSource -B $whisperBuild -DBUILD_SHARED_LIBS=OFF -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_EXAMPLES=ON -DGGML_NATIVE=OFF
    if ($LASTEXITCODE -ne 0) { throw "Failed to configure whisper.cpp." }
    cmake --build $whisperBuild --config Release --target whisper-cli
    if ($LASTEXITCODE -ne 0) { throw "Failed to build whisper-cli." }

    Copy-SingleMatch -SearchRoot $whisperBuild -FileName "whisper-cli.exe" -Destination $toolsDirectory
    Copy-Item -LiteralPath (Join-Path $whisperSource "LICENSE") -Destination (Join-Path $licenseDirectory "whisper.cpp-LICENSE") -Force

    $llamaArchive = Join-Path $workDirectory $llamaArchiveName
    Invoke-WebRequest -Uri $llamaUrl -OutFile $llamaArchive
    $llamaHash = (Get-FileHash -LiteralPath $llamaArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($llamaHash -ne $llamaSha256) {
        throw "llama.cpp archive checksum mismatch. Expected $llamaSha256, received $llamaHash."
    }
    $llamaDirectory = Join-Path $workDirectory "llama"
    Expand-Archive -LiteralPath $llamaArchive -DestinationPath $llamaDirectory
    # llama-completion needs its DLLs next to it, so copy the archive's files flat into tools.
    Get-ChildItem -Path $llamaDirectory -File -Recurse | Where-Object { $_.Extension -in ".exe", ".dll" -and $_.Name -notmatch "^(ffmpeg|ffprobe|whisper-cli)" } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $toolsDirectory -Force
    }
    if (-not (Test-Path -LiteralPath (Join-Path $toolsDirectory "llama-completion.exe"))) {
        throw "llama-completion.exe was not found in the llama.cpp archive."
    }
    $llamaLicense = @(Get-ChildItem -Path $llamaDirectory -Filter "LICENSE*" -File -Recurse | Select-Object -First 1)
    if ($llamaLicense.Count -eq 1) {
        Copy-Item -LiteralPath $llamaLicense[0].FullName -Destination (Join-Path $licenseDirectory "llama.cpp-LICENSE") -Force
    }

    New-Item -ItemType Directory -Path $llmDirectory -Force | Out-Null
    $llmModelPath = Join-Path $llmDirectory $llmModelName
    if (-not (Test-Path -LiteralPath $llmModelPath) -or
        (Get-FileHash -LiteralPath $llmModelPath -Algorithm SHA256).Hash.ToLowerInvariant() -ne $llmModelSha256) {
        Invoke-WebRequest -Uri $llmModelUrl -OutFile $llmModelPath
        $modelHash = (Get-FileHash -LiteralPath $llmModelPath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($modelHash -ne $llmModelSha256) {
            Remove-Item -LiteralPath $llmModelPath -Force
            throw "LLM model checksum mismatch. Expected $llmModelSha256, received $modelHash."
        }
    }

    & (Join-Path $toolsDirectory "llama-completion.exe") --version
    if ($LASTEXITCODE -ne 0) { throw "Bundled llama-completion.exe failed its version check." }
    & (Join-Path $toolsDirectory "ffmpeg.exe") -version | Select-Object -First 1
    if ($LASTEXITCODE -ne 0) { throw "Bundled ffmpeg.exe failed its version check." }
    & (Join-Path $toolsDirectory "ffprobe.exe") -version | Select-Object -First 1
    if ($LASTEXITCODE -ne 0) { throw "Bundled ffprobe.exe failed its version check." }
    & (Join-Path $toolsDirectory "whisper-cli.exe") -h | Select-Object -First 1
    if ($LASTEXITCODE -ne 0) { throw "Bundled whisper-cli.exe failed its help check." }
}
finally {
    $resolvedWorkDirectory = [System.IO.Path]::GetFullPath($workDirectory)
    if ($resolvedWorkDirectory.StartsWith($temporaryRoot, [System.StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolvedWorkDirectory).StartsWith("dropcut-tools-")) {
        Remove-Item -LiteralPath $resolvedWorkDirectory -Recurse -Force -ErrorAction SilentlyContinue
    }
}
