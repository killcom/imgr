# Make the release zip from a release build:
#   cargo build --release
#   pwsh tools/package.ps1 [-Tag v0.13]
# Writes dist/imgr_v<version>_win64.zip and dist/notes.md (that version's part of CHANGELOG.md).
# With -Tag, the tag must match the version in Cargo.toml (v0.13 = 0.13 or 0.13.0).
# Used by .github/workflows/release.yml; can be run by hand the same way.
param([string]$Tag = '')
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$cargo = Get-Content -LiteralPath 'Cargo.toml' -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([0-9.]+)"') { throw 'no version in Cargo.toml' }
$full = $Matches[1]
$short = $full -replace '\.0$', ''
if ($Tag) {
    $t = $Tag -replace '^v', ''
    if ($t -ne $full -and $t -ne $short) { throw "tag $Tag does not match Cargo.toml version $full" }
}

$exe = 'target/release/imgr.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "no $exe - run cargo build --release first" }

$name = "imgr_v${short}_win64"
$dist = Join-Path $repo 'dist'
$stage = Join-Path $dist $name
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage 'docs') | Out-Null
Copy-Item -LiteralPath $exe (Join-Path $stage 'imgr.exe')
foreach ($f in 'LICENSE', 'README.md', 'README_ko.md', 'CHANGELOG.md') { Copy-Item -LiteralPath $f $stage }
foreach ($f in 'usage.md', 'usage_ko.md') { Copy-Item -LiteralPath "docs/$f" (Join-Path $stage 'docs') }
Copy-Item -LiteralPath 'tools/imgr_ctl.py' $stage

$zip = Join-Path $dist "$name.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip

# release notes = the "## v<short>" part of CHANGELOG.md
$lines = Get-Content -LiteralPath 'CHANGELOG.md' -Encoding UTF8
$out = New-Object System.Collections.Generic.List[string]
$on = $false
foreach ($l in $lines) {
    if ($l -match '^## ') {
        if ($on) { break }
        $on = ($l.Trim() -eq "## v$short")
        continue
    }
    if ($on) { $out.Add($l) }
}
if ($out.Count -eq 0) { throw "CHANGELOG.md has no '## v$short' section" }
$notes = ($out -join "`n").Trim() + "`n`n" +
    "Download ``$name.zip``, unzip it anywhere and run ``imgr.exe``. Windows may warn that the app is unrecognized (it is not code-signed): More info > Run anyway.`n" +
    "``$name.zip`` 를 받아 아무 곳에 풀고 ``imgr.exe`` 를 실행하세요. 서명이 없는 프로그램이라 윈도우가 경고하면: 추가 정보 > 실행.`n"
[System.IO.File]::WriteAllText((Join-Path $dist 'notes.md'), $notes, (New-Object System.Text.UTF8Encoding($false)))

"zip:   $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
"notes: $(Join-Path $dist 'notes.md')"
