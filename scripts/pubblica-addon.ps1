# Pubblica il catalogo degli add-on, dopo il merge di una pull request.
#
#   powershell -ExecutionPolicy Bypass -File scripts\pubblica-addon.ps1
#
# 1. controlla tutti gli add-on in addons\ e li impacchetta in dist-addons\
#    (node scripts\addons.mjs pack): <id>-<versione>.zip e addons.json;
# 2. firma addons.json con la chiave degli add-on (~\.tauri\postamon-addons.key,
#    diversa da quella degli aggiornamenti di Postamon);
# 3. carica tutto nella release "addons" del repo su GitHub, da cui il
#    marketplace di Postamon legge il catalogo. Prima i pacchetti, poi il
#    catalogo e la sua firma: un Postamon che legge proprio in quel momento
#    trova una firma che non torna e riprova, non installa niente di sbagliato.
#
# Pubblicare va fatto dal branch main aggiornato: il catalogo contiene gli
# add-on così come sono nella cartella addons\ di adesso.
#
# -SenzaInvio si ferma dopo la firma (per controllare i pacchetti).

param(
    [string]$ChiaveFirma = "$HOME\.tauri\postamon-addons.key",
    [string]$Repo = "Xp67/Postamon",
    [string]$Tag = "addons",
    [switch]$SenzaInvio
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root "dist-addons"

function Fail($text) {
    Write-Host $text -ForegroundColor Red
    exit 1
}

if (-not (Test-Path $ChiaveFirma)) { Fail "Manca la chiave degli add-on: $ChiaveFirma" }

if (-not $SenzaInvio) {
    $branch = (git -C $root rev-parse --abbrev-ref HEAD).Trim()
    if ($branch -ne "main") { Fail "Sei sul branch ${branch}: il catalogo si pubblica da main" }
    $dirty = git -C $root status --porcelain -- addons sdk
    if ($dirty) { Fail "Ci sono modifiche non committate in addons\ o sdk\: committale prima di pubblicare" }
}

# ---- 1. pacchetti ----

if (Test-Path $out) { Remove-Item $out -Recurse -Force }
node (Join-Path $root "scripts\addons.mjs") pack $out
if ($LASTEXITCODE -ne 0) { Fail "Controllo degli add-on non superato: niente da pubblicare" }

# ---- 2. firma ----

$catalog = Join-Path $out "addons.json"
$env:CI = "true"
npx tauri signer sign -f $ChiaveFirma $catalog | Out-Null
if ($LASTEXITCODE -ne 0 -or -not (Test-Path "$catalog.sig")) { Fail "Firma del catalogo non riuscita" }
Write-Host "Catalogo firmato: $catalog"

if ($SenzaInvio) { exit 0 }

# ---- 3. GitHub ----

gh release view $Tag -R $Repo *> $null
if ($LASTEXITCODE -ne 0) {
    gh release create $Tag -R $Repo --title "Add-on" --latest=false `
        --notes "Catalogo degli add-on di Postamon. Non serve scaricare niente da qui: si installano dal marketplace dentro Postamon (icona del puzzle)."
    if ($LASTEXITCODE -ne 0) { Fail "Release $Tag non creata" }
}
$packages = Get-ChildItem $out -Filter *.zip | ForEach-Object { $_.FullName }
if ($packages) {
    gh release upload $Tag -R $Repo --clobber @packages
    if ($LASTEXITCODE -ne 0) { Fail "Invio dei pacchetti non riuscito" }
}
gh release upload $Tag -R $Repo --clobber $catalog "$catalog.sig"
if ($LASTEXITCODE -ne 0) { Fail "Invio del catalogo non riuscito" }
Write-Host "Catalogo pubblicato: https://github.com/$Repo/releases/tag/$Tag" -ForegroundColor Green
