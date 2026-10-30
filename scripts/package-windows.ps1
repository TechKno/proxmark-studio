$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo build --locked --release --target x86_64-pc-windows-msvc --bins
if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }
$metadata = cargo metadata --locked --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Cannot read package version' }
$version = (($metadata | ConvertFrom-Json).packages | Where-Object name -eq 'proxmark-studio').version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Unexpected package version' }
$packageDir = "dist/ProxmarkStudio-$version-Windows-x64"
New-Item -ItemType Directory -Force -Path $packageDir | Out-Null
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
foreach ($binary in @('proxmark-studio.exe', 'mock-pm3.exe')) {
    $source = Join-Path $targetDir "x86_64-pc-windows-msvc/release/$binary"
    $destination = Join-Path $packageDir $binary
    if (!(Test-Path -LiteralPath $destination) -or
        (Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $destination).Hash) {
        Copy-Item -LiteralPath $source -Destination $destination
    }
}
Copy-Item 'README.md', 'LICENSE', 'VALIDATION.md', 'ORGANISATIONS.md', 'SECURITY.md' $packageDir
New-Item -ItemType Directory -Force -Path "$packageDir/examples" | Out-Null
Copy-Item 'examples/synthetic-users.csv', 'examples/synthetic-organisation-profile.json' "$packageDir/examples"
Compress-Archive -Path "$packageDir/*" -DestinationPath "$packageDir.zip" -Force
