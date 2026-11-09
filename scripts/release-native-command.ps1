# Windows PowerShell 5.1 wraps redirected native stderr in ErrorRecord objects.
# Scope Continue to the invocation only; callers must check the captured exit code.
function Invoke-ReleaseNative {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)][string]$FilePath,
        [string[]]$ArgumentList = @()
    )
    $command = Get-Command -Name $FilePath -CommandType Application -ErrorAction Stop | Select-Object -First 1
    $savedPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = @(& $command.Source @ArgumentList 2>&1)
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $savedPreference
    }
    [pscustomobject]@{
        ExitCode = $exitCode
        Output = @($output | ForEach-Object { $_.ToString() })
    }
}