# Run only on an elevated, disposable Windows test runner. All data is synthetic.
$ErrorActionPreference = 'Stop'
$shareName = 'mcpstack-' + [Guid]::NewGuid().ToString('N')
$fixturePath = Join-Path ([IO.Path]::GetTempPath()) $shareName
$account = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$remotePath = "\\localhost\$shareName"
$usedDrives = @([IO.DriveInfo]::GetDrives().Name | ForEach-Object { $_.Substring(0, 2) })
$drive = @('Z:', 'Y:', 'X:', 'W:') | Where-Object { $_ -notin $usedDrives } | Select-Object -First 1
if (!$drive) { throw 'No free drive letter for the SMB fixture.' }
$shareCreated = $false
$driveMapped = $false
try {
    New-Item -ItemType Directory -Path $fixturePath | Out-Null
    New-SmbShare -Name $shareName -Path $fixturePath -FullAccess $account | Out-Null
    $shareCreated = $true
    & net.exe use $drive $remotePath /persistent:no
    if ($LASTEXITCODE -ne 0) { throw 'Could not map the SMB test drive.' }
    $driveMapped = $true
    $env:MCPSTACK_TEST_SMB_UNC = $remotePath
    $env:MCPSTACK_TEST_SMB_DRIVE = "$drive\"
    & cargo test --locked smb_imports_support_unc_and_mapped_drive_paths -- --ignored
    if ($LASTEXITCODE -ne 0) { throw 'SMB import tests failed.' }
} finally {
    Remove-Item Env:MCPSTACK_TEST_SMB_UNC, Env:MCPSTACK_TEST_SMB_DRIVE -ErrorAction SilentlyContinue
    if ($driveMapped) { & net.exe use $drive /delete /y }
    if ($shareCreated) { Remove-SmbShare -Name $shareName -Force }
    if (Test-Path $fixturePath) { Remove-Item -LiteralPath $fixturePath -Recurse -Force }
}
