param([string]$Target="all")
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Src = Join-Path $Root "skills/context-engine/SKILL.md"
function Install-Codex { New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.codex\skills\context-engine" | Out-Null; Copy-Item $Src "$env:USERPROFILE\.codex\skills\context-engine\SKILL.md" -Force; Write-Host "installed Codex skill" }
function Install-OpenCode { New-Item -ItemType Directory -Force -Path "$Root\.opencode\skills\context-engine" | Out-Null; Copy-Item $Src "$Root\.opencode\skills\context-engine\SKILL.md" -Force; Write-Host "installed OpenCode skill" }
function Install-Claude { New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\skills\context-engine" | Out-Null; Copy-Item $Src "$env:USERPROFILE\.claude\skills\context-engine\SKILL.md" -Force; Write-Host "installed Claude skill" }
switch ($Target) {
  "--codex" { Install-Codex }
  "--opencode" { Install-OpenCode }
  "--claude" { Install-Claude }
  default { Install-Codex; Install-OpenCode; Install-Claude }
}
