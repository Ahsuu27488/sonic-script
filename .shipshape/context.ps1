# shipshape session report, PowerShell twin of context.sh (for Windows machines without sh).
# Everything printed is meant for the agent's context at session start. Must stay fast and never fail.
# Project-specific lines: .shipshape/context.local.ps1 (dot-sourced at the end).

$ErrorActionPreference = 'SilentlyContinue'
$root = git rev-parse --show-toplevel 2>$null
if (-not $root) {
    Write-Output '## Project state (shipshape)'
    Write-Output 'Not a git repository yet. A new project starts with the shipshape-adopt skill (new-project flow).'
    exit 0
}
Set-Location $root

function Get-Cfg([string]$key) {
    if (-not (Test-Path '.shipshape/config')) { return '' }
    $line = Get-Content '.shipshape/config' | Where-Object { $_ -like "$key=*" } | Select-Object -Last 1
    if ($line) { return $line.Substring($key.Length + 1).Trim() } else { return '' }
}

$default = Get-Cfg 'default_branch'; if (-not $default) { $default = 'main' }
$codePaths = @((Get-Cfg 'code_paths') -split '\s+' | Where-Object { $_ })
$branch = git branch --show-current 2>$null
$hasHead = [bool](git rev-parse --verify -q HEAD 2>$null)

Write-Output '## Project state (shipshape session report)'
Write-Output ''
Write-Output "**Branch:** $(if ($branch) { $branch } else { 'detached HEAD' }) (default: $default)"
Write-Output ''
if ($hasHead) {
    Write-Output '**Recent commits:**'
    git log --oneline -8 2>$null | ForEach-Object { Write-Output "- $_" }
} else { Write-Output '**Commits:** none yet (new repository)' }
Write-Output ''

$dirty = @(git status --porcelain 2>$null)
if ($dirty.Count -gt 0) {
    Write-Output "**Uncommitted changes ($($dirty.Count))** - work was in progress; inspect before starting anything new:"
    $dirty | Select-Object -First 25 | ForEach-Object { Write-Output "    $_" }
} else { Write-Output '**Working tree:** clean' }
Write-Output ''

$warnings = @()
$missing = @('docs/STATUS.md', 'docs/DECISIONS.md', 'docs/ARCHITECTURE.md', 'docs/CHANGELOG.md') | Where-Object { -not (Test-Path $_) }
if ($missing.Count -gt 0) { $warnings += "Project memory is missing: $($missing -join ' '). Set it up from the real code (shipshape-adopt / shipshape-memory) before writing code." }

if ($branch -and $hasHead -and ($branch -eq $default -or $branch -eq 'main' -or $branch -eq 'master')) {
    $warnings += "You are on the default branch '$branch'. Before the first change, ask the user to create a branch (suggest a name such as feat/<topic>); never commit here."
}

$ahead = git rev-list --count '@{u}..HEAD' 2>$null
if ($ahead -and [int]$ahead -gt 0) { $warnings += "$ahead commit(s) are not pushed yet (git push)." }

if ((Test-Path 'docs/STATUS.md') -and $codePaths.Count -gt 0) {
    $statusCommit = git log -1 --format=%H -- docs/STATUS.md 2>$null
    if (-not $statusCommit) { $warnings += 'docs/STATUS.md has never been committed.' }
    else {
        $newer = @(git log --oneline "$statusCommit..HEAD" -- @codePaths 2>$null)
        if ($newer.Count -gt 0) {
            $warnings += "docs/STATUS.md is older than $($newer.Count) code commit(s): $(($newer | Select-Object -First 5) -join '; '). Bring the docs up to date (shipshape-memory) before new work."
        }
    }
    $dirtyFiles = $dirty | ForEach-Object { $_.Substring(3) }
    $dirtyCode = @($dirtyFiles | Where-Object { $f = $_; $codePaths | Where-Object { $f.StartsWith($_) } })
    if ($dirtyCode.Count -gt 0 -and -not ($dirtyFiles -contains 'docs/STATUS.md')) { $warnings += 'Uncommitted code changes exist but docs/STATUS.md is untouched.' }
}

if (Test-Path '.githooks') {
    $hp = git config --get core.hooksPath 2>$null
    if ($hp -ne '.githooks') { $warnings += "The repo's git hooks are NOT active in this clone (core.hooksPath='$hp'). Run: git config core.hooksPath .githooks" }
}
if (Test-Path '.shipshape/last-report.md') { $warnings += 'An unattended run left a report: .shipshape/last-report.md. Summarise it for the user.' }

if ($warnings.Count -gt 0) {
    Write-Output '**ATTENTION before writing code:**'
    $warnings | ForEach-Object { Write-Output "- $_" }
} else { Write-Output '**Memory docs:** in sync with the latest code commit.' }
Write-Output ''
Write-Output 'Before changing code: read docs/STATUS.md, docs/DECISIONS.md and docs/ARCHITECTURE.md, then the code you will touch.'

if (Test-Path '.shipshape/context.local.ps1') { . ./.shipshape/context.local.ps1 }
exit 0
