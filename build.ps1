# Forwards to scripts\build.ps1, the one Windows build recipe.
& "$PSScriptRoot\scripts\build.ps1" @args; exit $LASTEXITCODE
