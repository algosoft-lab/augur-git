param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerPath
)

$ErrorActionPreference = 'Stop'
$installDirectory = Join-Path $env:RUNNER_TEMP 'augur-cli-installer-test'
$cliPath = Join-Path $installDirectory 'bin\agit.exe'
$cliMetadataPath = 'Software\com.augur.git.tauri\Cli'

function Get-UserPathEntries {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  if ($null -eq $key) { return @() }
  try {
    $value = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    return @(([string]$value -split ';') | Where-Object { $_ })
  } finally {
    $key.Dispose()
  }
}

function Test-PathEntry([string]$Target) {
  $normalizedTarget = $Target.Trim().TrimEnd('\').ToLowerInvariant()
  foreach ($entry in Get-UserPathEntries) {
    if ($entry.Trim().Trim('"').TrimEnd('\').ToLowerInvariant() -eq $normalizedTarget) {
      return $true
    }
  }
  return $false
}

function Set-UserPathValue([string]$Value) {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
  if ($null -eq $key) { $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment') }
  try {
    try { $kind = $key.GetValueKind('Path') } catch { $kind = [Microsoft.Win32.RegistryValueKind]::String }
    $key.SetValue('Path', $Value, $kind)
  } finally {
    $key.Dispose()
  }
}

function Invoke-Installer([string[]]$ExtraArguments = @()) {
  $arguments = @('/S') + $ExtraArguments + @("/D=$installDirectory")
  $process = Start-Process -FilePath $InstallerPath -ArgumentList $arguments -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "Installer exited with status $($process.ExitCode)."
  }
}

function Invoke-Cli([string]$Argument) {
  $process = Start-Process -FilePath $cliPath -ArgumentList $Argument -Wait -PassThru -NoNewWindow
  if ($process.ExitCode -ne 0) {
    throw "agit $Argument exited with status $($process.ExitCode)."
  }
}

function Assert-ShellCanResolveAgit([string]$Name, [string]$Executable, [string[]]$Arguments) {
  $output = & $Executable @Arguments 2>&1
  if ($LASTEXITCODE -ne 0 -or ($output -join "`n") -notmatch 'Augur Git') {
    throw "$Name could not run agit --version through PATH. Output: $($output -join ' ')"
  }
}

if (Test-Path $installDirectory) {
  throw "The isolated installation directory already exists: $installDirectory"
}
if ([Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cliMetadataPath)) {
  throw 'CLI metadata already exists on this runner; refusing to overwrite it.'
}

try {
  Invoke-Installer
  if (!(Test-Path $cliPath)) { throw 'The installer did not place bin\agit.exe.' }
  if (!(Test-PathEntry (Join-Path $installDirectory 'bin'))) {
    throw 'A fresh install did not add its bin directory to the user PATH.'
  }
  $metadata = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cliMetadataPath)
  if ($null -eq $metadata -or $metadata.GetValue('Enabled') -ne 1) {
    throw 'A fresh install did not persist the enabled CLI choice.'
  }
  $metadata.Dispose()
  Invoke-Cli '--help'
  Invoke-Cli '--version'
  $originalProcessPath = $env:PATH
  try {
    $env:PATH = (Join-Path $installDirectory 'bin') + ';' + $originalProcessPath
    Assert-ShellCanResolveAgit 'PowerShell' (Get-Command powershell.exe -ErrorAction Stop).Source @('-NoProfile', '-NonInteractive', '-Command', 'agit --version')
    Assert-ShellCanResolveAgit 'Command Prompt' (Get-Command cmd.exe -ErrorAction Stop).Source @('/d', '/c', 'agit --version')
    Assert-ShellCanResolveAgit 'Git Bash' (Get-Command bash.exe -ErrorAction Stop).Source @('--noprofile', '--norc', '-c', 'agit --version')
  } finally {
    $env:PATH = $originalProcessPath
  }

  Invoke-Installer @('/UPDATE')
  $metadata = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cliMetadataPath)
  if ($null -eq $metadata -or $metadata.GetValue('Enabled') -ne 1) {
    throw 'A passive update did not preserve the enabled CLI choice.'
  }
  $metadata.Dispose()
  if (!(Test-PathEntry (Join-Path $installDirectory 'bin'))) {
    throw 'A passive update removed the CLI PATH entry.'
  }

  $environmentKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  $userPath = $environmentKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  $environmentKey.Dispose()
  $entries = @(([string]$userPath -split ';') | Where-Object {
    $_.Trim().Trim('"').TrimEnd('\').ToLowerInvariant() -ne (Join-Path $installDirectory 'bin').TrimEnd('\').ToLowerInvariant()
  })
  Set-UserPathValue ($entries -join ';')
  [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($cliMetadataPath)
  Invoke-Installer @('/UPDATE')
  $metadata = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cliMetadataPath)
  if ($null -eq $metadata -or $metadata.GetValue('Enabled') -ne 0) {
    throw 'An upgrade without a saved CLI choice did not default to disabled.'
  }
  $metadata.Dispose()
  if (Test-PathEntry (Join-Path $installDirectory 'bin')) {
    throw 'An upgrade without a saved CLI choice unexpectedly modified PATH.'
  }

  Invoke-Cli '--internal-cli-register'
  if (!(Test-PathEntry (Join-Path $installDirectory 'bin'))) {
    throw 'Settings-style CLI registration did not restore the user PATH entry.'
  }
  $uninstaller = Join-Path $installDirectory 'uninstall.exe'
  $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw "Uninstaller exited with status $($process.ExitCode)." }
  if (Test-PathEntry (Join-Path $installDirectory 'bin')) {
    throw 'Uninstall left the application-owned PATH entry behind.'
  }
  if ([Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cliMetadataPath)) {
    throw 'Uninstall left application CLI metadata behind.'
  }
  if (Test-Path $cliPath) { throw 'Uninstall left the bundled CLI in place.' }
} finally {
  if (Test-Path $cliPath) {
    try { Start-Process -FilePath $cliPath -ArgumentList '--internal-cli-uninstall-package' -Wait -NoNewWindow | Out-Null } catch {}
  }
  if (Test-Path $installDirectory) { Remove-Item -LiteralPath $installDirectory -Recurse -Force }
}

Write-Output 'Windows installer CLI registration checks passed.'
