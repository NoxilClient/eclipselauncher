param([Parameter(Mandatory = $true)][string]$Path)

$ErrorActionPreference = 'Stop'

# Senza il certificato nei secret la build resta non firmata, senza far fallire nulla.
if (-not $env:WINDOWS_SIGN_PFX_BASE64) {
    Write-Host "sign-windows: nessun certificato configurato, $Path resta non firmato"
    exit 0
}

if (-not (Test-Path $Path)) {
    throw "sign-windows: file inesistente: $Path"
}

$pfx = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.pfx')
[IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_SIGN_PFX_BASE64))

try {
    $signtool = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $signtool) { throw "sign-windows: signtool.exe non trovato nel Windows SDK" }

    & $signtool.FullName sign /fd SHA256 /f $pfx /p $env:WINDOWS_SIGN_PASSWORD `
        /tr http://timestamp.digicert.com /td SHA256 $Path
    if ($LASTEXITCODE -ne 0) { throw "sign-windows: signtool ha fallito su $Path (exit $LASTEXITCODE)" }
    Write-Host "sign-windows: firmato $Path"
}
finally {
    Remove-Item $pfx -Force -ErrorAction SilentlyContinue
}
