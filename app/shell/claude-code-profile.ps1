Write-Host "You can launch claude or codex here."

function _xmlui_mark_agent {
    param([string]$Provider)
    $hint = if ($env:BRAM_AGENT_HINT) { $env:BRAM_AGENT_HINT } else { $env:XMLUI_DESKTOP_AGENT_HINT }
    if (-not $hint) { return }
    $parent = Split-Path -Parent $hint
    if ($parent) {
        try { New-Item -ItemType Directory -Force -Path $parent | Out-Null } catch { return }
    }
    Set-Content -Path $hint -Value ('{"provider":"' + $Provider + '"}')
}

function _xmlui_has_repo_setup {
    return (Test-Path "resources/.worklist-authorization.json") -or (Test-Path ".claude/bram-conventions.md") -or (Test-Path ".claude/xmlui-desktop-conventions.md")
}

function _xmlui_codex_seed_text {
    $path = Join-Path $PSScriptRoot "codex-startup-instructions.md"
    if (-not (Test-Path $path)) { return $null }
    return [System.IO.File]::ReadAllText($path).TrimEnd()
}

function _xmlui_codex_is_subcommand {
    param([string]$Arg)
    switch ($Arg) {
        "exec" { return $true }
        "review" { return $true }
        "login" { return $true }
        "logout" { return $true }
        "mcp" { return $true }
        "plugin" { return $true }
        "mcp-server" { return $true }
        "app-server" { return $true }
        "remote-control" { return $true }
        "app" { return $true }
        "completion" { return $true }
        "update" { return $true }
        "sandbox" { return $true }
        "debug" { return $true }
        "apply" { return $true }
        "resume" { return $true }
        "fork" { return $true }
        "cloud" { return $true }
        "exec-server" { return $true }
        "features" { return $true }
        "help" { return $true }
        default { return $false }
    }
}

function _xmlui_pick_agent {
    $provider = if ($env:BRAM_STARTUP_AGENT) { $env:BRAM_STARTUP_AGENT } elseif ($env:XMLUI_DESKTOP_STARTUP_AGENT) { $env:XMLUI_DESKTOP_STARTUP_AGENT } else { $null }
    if ($provider) { return $provider }
    if (Get-Command -Name codex -CommandType Application -ErrorAction SilentlyContinue) { return "codex" }
    return "claude"
}

function _xmlui_run_real {
    param([string]$Name, [object[]]$ForwardArgs)
    $real = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $real) {
        Write-Error "$Name not found on PATH"
        return
    }
    & $real.Source @ForwardArgs
}

function agent {
    param([Parameter(ValueFromRemainingArguments = $true)][object[]]$Args)
    $provider = _xmlui_pick_agent
    switch ($provider) {
        "codex" {
            if ($Args.Count -gt 0 -and [string]$Args[0] -eq "--continue") {
                $remaining = @()
                if ($Args.Count -gt 1) {
                    $remaining = $Args[1..($Args.Count - 1)]
                }
                codex resume @remaining
                return
            }
            codex @Args
            return
        }
        default {
            claude @Args
            return
        }
    }
}

function claude {
    _xmlui_mark_agent claude
    _xmlui_run_real claude $args
}

function codex {
    _xmlui_mark_agent codex
    if (-not (_xmlui_has_repo_setup)) {
        _xmlui_run_real codex $args
        return
    }
    $seedText = _xmlui_codex_seed_text
    if (-not $seedText) {
        _xmlui_run_real codex $args
        return
    }
    if ($args.Count -eq 0) {
        _xmlui_run_real codex @($seedText)
        return
    }
    $forward = New-Object System.Collections.Generic.List[object]
    $expectValue = $false
    $prompt = $null
    foreach ($arg in $args) {
        if ($expectValue) {
            $forward.Add($arg)
            $expectValue = $false
            continue
        }
        if ($arg -is [string]) {
            switch -Regex ($arg) {
                '^(--config|--enable|--disable|--image|--model|--profile|--sandbox|--cd|--add-dir|--ask-for-approval|--remote|--remote-auth-token-env|--local-provider|--output-schema|--output-last-message|-c|-i|-m|-p|-s|-C|-a|-o)$' {
                    $forward.Add($arg)
                    $expectValue = $true
                    continue
                }
                '^--[^=]+=.*$' {
                    $forward.Add($arg)
                    continue
                }
                '^-' {
                    $forward.Add($arg)
                    continue
                }
                default {
                    if (_xmlui_codex_is_subcommand $arg) {
                        _xmlui_run_real codex $args
                        return
                    }
                    $prompt = [string]$arg
                    break
                }
            }
        } else {
            $prompt = [string]$arg
            break
        }
    }
    if ($prompt) {
        _xmlui_run_real codex ($forward + @($seedText + "`n`nAdditional user request: " + $prompt))
    } else {
        _xmlui_run_real codex ($forward + @($seedText))
    }
}
