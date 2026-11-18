<#
Publishes one of your mods to ModNest. No server: the zip goes to a GitHub Release,
and registry/registry.json (which every ModNest reads) gets the new version.

  .\scripts\publish-mod.ps1 -Id chainedforge -Version 1.1.0 -Source "C:\path\to\ChainedForge-output"

-Source can be a folder (it gets zipped) or a ready .zip.
What goes in the zip depends on the mod's installType in registry.json:
  plugin  -> the files that belong in BepInEx\plugins\<installFolder>\  (e.g. ChainedForge.dll + packs\)
  cf-pack -> a ChainedForge pack folder (pack.json at the top)
  root    -> files that belong in the game folder itself
New mod? Add an entry to registry.json first (copy an existing one), then run this.
#>
param(
  [Parameter(Mandatory)] [string] $Id,
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Source,
  [string] $Repo = '104316stu/modnest',
  [switch] $NoPush
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$regPath = Join-Path $root 'registry\registry.json'
$reg = Get-Content $regPath -Raw | ConvertFrom-Json
$pkg = $reg.packages | Where-Object { $_.id -eq $Id }
if (-not $pkg) { throw "No package '$Id' in registry.json. Add it first." }

$tag = "$Id-$Version"
$work = Join-Path ([IO.Path]::GetTempPath()) "modnest-publish-$tag"
New-Item -ItemType Directory -Force $work | Out-Null
$zip = Join-Path $work "$tag.zip"
if (Test-Path $zip) { Remove-Item $zip }

if ((Get-Item $Source).PSIsContainer) {
  # Windows' tar writes standard zip paths (forward slashes), unlike Compress-Archive on PowerShell 5.
  Push-Location $Source
  try { tar.exe -a -c -f $zip * ; if ($LASTEXITCODE) { throw 'zipping failed' } } finally { Pop-Location }
} else {
  Copy-Item $Source $zip
}
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Write-Host "Zip: $zip ($([math]::Round((Get-Item $zip).Length / 1KB)) KB)  sha256 $hash"

gh release create $tag $zip --repo $Repo --title "$($pkg.name) $Version" --notes "Mod package for ModNest." --latest=false
if ($LASTEXITCODE) { throw 'gh release create failed' }

$pkg.version = $Version
$pkg.download = "https://github.com/$Repo/releases/download/$tag/$tag.zip"
if ($pkg.PSObject.Properties.Name -contains 'sha256') { $pkg.sha256 = $hash } else { $pkg | Add-Member sha256 $hash }
$reg.updated = (Get-Date -Format 'yyyy-MM-dd')
$json = $reg | ConvertTo-Json -Depth 10
[IO.File]::WriteAllText($regPath, $json, (New-Object Text.UTF8Encoding $false))

if (-not $NoPush) {
  git -C $root add registry/registry.json
  git -C $root commit -m "Publish $($pkg.name) $Version"
  git -C $root push
}
Write-Host "Done. Everyone's ModNest will see $($pkg.name) $Version next time they open the game."
