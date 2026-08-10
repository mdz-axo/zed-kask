[CmdletBinding()]
Param()

$ErrorActionPreference = 'Stop'
[Console]::Error.WriteLine('error: script/bundle-windows.ps1 is disabled in zed-kask')
[Console]::Error.WriteLine('zed-kask does not publish Windows Zed application installers or file-association packages.')
exit 1
