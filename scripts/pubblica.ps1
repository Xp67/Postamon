# Pubblica una nuova versione di Postamon.
#
#   powershell -ExecutionPolicy Bypass -File scripts\pubblica.ps1 -Versione 0.2.0 -Note "Ricerca nella posta"
#
# 1. scrive la versione in tauri.conf.json, Cargo.toml e package.json;
# 2. compila l'installer firmato con la chiave degli aggiornamenti
#    (~\.tauri\postamon.key) in src-tauri\target\dist, così non tocca un
#    Postamon aperto da src-tauri\target\release;
# 3. carica installer e latest.json sul server della chat interna, da cui i
#    Postamon installati si aggiornano da soli (latest.json per ultimo: nessun
#    PC vede la nuova versione prima che l'installer sia arrivato tutto).
#
# -SenzaInvio si ferma dopo la compilazione.
#
# Server, utente e chiave SSH, se non passati, vengono da postamon.local.json
# (fuori da git, modello in postamon.esempio.json); da lì la compilazione
# prende anche il server della chat proposto ai nuovi Postamon.

param(
    [Parameter(Mandatory = $true)][string]$Versione,
    [string]$Note = "",
    [string]$Server = "",
    [string]$Utente = "",
    [string]$ChiaveSsh = "",
    [string]$ChiaveFirma = "$HOME\.tauri\postamon.key",
    [string]$Cartella = "postamon-interno/dati/aggiornamenti",
    [switch]$SenzaInvio
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$utf8 = New-Object System.Text.UTF8Encoding $false

function Fail($text) {
    Write-Host $text -ForegroundColor Red
    exit 1
}

if ($Versione -notmatch '^\d+\.\d+\.\d+$') { Fail "Versione non valida: usa il formato 0.2.0" }
if (-not (Test-Path $ChiaveFirma)) { Fail "Manca la chiave degli aggiornamenti: $ChiaveFirma" }

$localConf = Join-Path $root "postamon.local.json"
if (Test-Path $localConf) {
    $local = [IO.File]::ReadAllText($localConf) | ConvertFrom-Json
    if (-not $Server) { $Server = $local.serverChat -replace ':\d+$', '' }
    if (-not $Utente) { $Utente = $local.utenteSsh }
    if (-not $ChiaveSsh) { $ChiaveSsh = $local.chiaveSsh -replace '^~', $HOME }
} else {
    Write-Host "Manca postamon.local.json: i nuovi Postamon non avranno un server della chat predefinito" -ForegroundColor Yellow
}
if (-not $SenzaInvio -and (-not $Server -or -not $Utente -or -not $ChiaveSsh)) {
    Fail "Server, utente o chiave SSH mancanti: compila postamon.local.json (vedi postamon.esempio.json)"
}

# ---- 1. versione ----

$conf = Join-Path $root "src-tauri\tauri.conf.json"
$text = [IO.File]::ReadAllText($conf)
$attuale = [regex]::Match($text, '"version": "([^"]+)"').Groups[1].Value
if ([version]$Versione -lt [version]$attuale) { Fail "La versione $Versione è più vecchia di quella attuale ($attuale)" }

function Set-Version($path, $pattern, $replacement) {
    $content = [IO.File]::ReadAllText($path)
    $content = ([regex]$pattern).Replace($content, $replacement, 1)
    [IO.File]::WriteAllText($path, $content, $utf8)
}
Set-Version $conf '"version": "[^"]+"' "`"version`": `"$Versione`""
Set-Version (Join-Path $root "package.json") '"version": "[^"]+"' "`"version`": `"$Versione`""
Set-Version (Join-Path $root "src-tauri\Cargo.toml") '(?m)^version = "[^"]+"' "version = `"$Versione`""
Write-Host "Versione $Versione (era $attuale)"

# ---- 2. installer firmato ----

$env:TAURI_SIGNING_PRIVATE_KEY = [IO.File]::ReadAllText($ChiaveFirma).Trim()
$env:CARGO_TARGET_DIR = Join-Path $root "src-tauri\target\dist"
Push-Location $root
try {
    npx tauri build
    if ($LASTEXITCODE -ne 0) { Fail "Compilazione non riuscita" }
} finally {
    Pop-Location
}

$bundle = Join-Path $env:CARGO_TARGET_DIR "release\bundle\nsis"
$exe = Join-Path $bundle "Postamon_${Versione}_x64-setup.exe"
$sig = "$exe.sig"
if (-not (Test-Path $exe) -or -not (Test-Path $sig)) { Fail "Installer o firma non trovati in $bundle" }

$manifest = [ordered]@{
    version   = $Versione
    notes     = $Note
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = @{
        "windows-x86_64" = [ordered]@{
            signature = [IO.File]::ReadAllText($sig).Trim()
            # Solo il nome: il server lo completa con il suo indirizzo.
            url       = Split-Path $exe -Leaf
        }
    }
}
$latest = Join-Path $bundle "latest.json"
[IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5), $utf8)
Write-Host "Installer: $exe"

if ($SenzaInvio) { exit 0 }

# ---- 3. sul server ----

$dest = "$Utente@$Server"
ssh -i $ChiaveSsh $dest "mkdir -p $Cartella"
if ($LASTEXITCODE -ne 0) { Fail "Server $Server non raggiungibile" }
scp -i $ChiaveSsh $exe "${dest}:$Cartella/"
if ($LASTEXITCODE -ne 0) { Fail "Invio dell'installer non riuscito" }
scp -i $ChiaveSsh $latest "${dest}:$Cartella/latest.json"
if ($LASTEXITCODE -ne 0) { Fail "Invio di latest.json non riuscito" }
Write-Host "Pubblicata la versione $Versione su $Server" -ForegroundColor Green
