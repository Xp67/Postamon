# Pubblica una nuova versione di Postamon.
#
#   powershell -ExecutionPolicy Bypass -File scripts\pubblica.ps1 -Versione 0.2.0 -Note "Ricerca nella posta"
#
# 1. scrive la versione in tauri.conf.json, Cargo.toml, package.json e
#    package-lock.json;
# 2. compila l'installer firmato con la chiave degli aggiornamenti
#    (~\.tauri\postamon.key) in src-tauri\target\dist, così non tocca un
#    Postamon aperto da src-tauri\target\release;
# 3. carica installer e latest.json sul server della chat interna, da cui i
#    Postamon installati si aggiornano da soli (latest.json per ultimo: nessun
#    PC vede la nuova versione prima che l'installer sia arrivato tutto).
#
# -SenzaInvio si ferma dopo la compilazione; -SoloInvio carica una versione
# già compilata (è come lo usa `scripts/pm ship`, che fra i due passi fa commit,
# tag e push). Sul server il latest.json precedente resta in latest.prima.json,
# per `pm rollback app`.
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
    [switch]$SenzaInvio,
    [switch]$SoloInvio
)

$ErrorActionPreference = "Stop"
# Lanciato da pm (Git Bash) i messaggi devono uscire in UTF-8, se no le lettere accentate si storpiano.
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$root = Split-Path $PSScriptRoot -Parent
$utf8 = New-Object System.Text.UTF8Encoding $false

function Fail($text) {
    Write-Host $text -ForegroundColor Red
    exit 1
}

if ($Versione -notmatch '^\d+\.\d+\.\d+$') { Fail "Versione non valida: usa il formato 0.2.0" }
if ($SenzaInvio -and $SoloInvio) { Fail "-SenzaInvio e -SoloInvio insieme non fanno niente" }
if (-not $SoloInvio -and -not (Test-Path $ChiaveFirma)) { Fail "Manca la chiave degli aggiornamenti: $ChiaveFirma" }

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

$target = Join-Path $root "src-tauri\target\dist"
$bundle = Join-Path $target "release\bundle\nsis"
$exe = Join-Path $bundle "Postamon_${Versione}_x64-setup.exe"
$sig = "$exe.sig"
$latest = Join-Path $bundle "latest.json"

if ($SoloInvio) {
    if (-not (Test-Path $exe) -or -not (Test-Path $sig) -or -not (Test-Path $latest)) {
        Fail "La versione $Versione non è compilata in $bundle"
    }
    $pronta = ([IO.File]::ReadAllText($latest) | ConvertFrom-Json).version
    if ($pronta -ne $Versione) { Fail "latest.json in $bundle è della versione $pronta, non della $Versione" }
}

# ---- 1. versione ----

if (-not $SoloInvio) {
    $conf = Join-Path $root "src-tauri\tauri.conf.json"
    $text = [IO.File]::ReadAllText($conf)
    $attuale = [regex]::Match($text, '"version": "([^"]+)"').Groups[1].Value
    if ([version]$Versione -lt [version]$attuale) { Fail "La versione $Versione è più vecchia di quella attuale ($attuale)" }

    function Set-Version($path, $pattern, $replacement, $count = 1) {
        $content = [IO.File]::ReadAllText($path)
        $content = ([regex]$pattern).Replace($content, $replacement, $count)
        [IO.File]::WriteAllText($path, $content, $utf8)
    }
    Set-Version $conf '"version": "[^"]+"' "`"version`": `"$Versione`""
    Set-Version (Join-Path $root "package.json") '"version": "[^"]+"' "`"version`": `"$Versione`""
    # In package-lock.json la versione compare due volte: in cima e nel pacchetto radice.
    Set-Version (Join-Path $root "package-lock.json") '"version": "[^"]+"' "`"version`": `"$Versione`"" 2
    Set-Version (Join-Path $root "src-tauri\Cargo.toml") '(?m)^version = "[^"]+"' "version = `"$Versione`""
    Write-Host "Versione $Versione (era $attuale)"

    # ---- 2. installer firmato ----

    # La chiave ha la password vuota, ma se TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    # manca Tauri la chiede a terminale. PowerShell non sa creare una variabile
    # vuota ($env:X = "" la cancella): la compilazione parte con ProcessStartInfo,
    # che la passa, e senza npx, che la ripasserebbe da un'altra shell.
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = (Get-Command node).Source
    $psi.Arguments = '"' + (Join-Path $root "node_modules\@tauri-apps\cli\tauri.js") + '" build'
    $psi.WorkingDirectory = $root
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables["TAURI_SIGNING_PRIVATE_KEY"] = [IO.File]::ReadAllText($ChiaveFirma).Trim()
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        $psi.EnvironmentVariables["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"] = ""
    }
    $psi.EnvironmentVariables["CARGO_TARGET_DIR"] = $target
    $build = [System.Diagnostics.Process]::Start($psi)
    $build.WaitForExit()
    if ($build.ExitCode -ne 0) { Fail "Compilazione non riuscita" }

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
    [IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5), $utf8)
    Write-Host "Installer: $exe"
}

if ($SenzaInvio) { exit 0 }

# ---- 3. sul server ----

$dest = "$Utente@$Server"
ssh -i $ChiaveSsh $dest "mkdir -p $Cartella"
if ($LASTEXITCODE -ne 0) { Fail "Server $Server non raggiungibile" }
scp -i $ChiaveSsh $exe "${dest}:$Cartella/"
if ($LASTEXITCODE -ne 0) { Fail "Invio dell'installer non riuscito" }
# La versione che c'era resta in latest.prima.json (pm rollback app). Se si
# ricarica la stessa versione la copia non si tocca, se no si perderebbe.
# Niente virgolette doppie nel comando: PowerShell 5 non le passa intere a ssh.
ssh -i $ChiaveSsh $dest "cd $Cartella && if [ -f latest.json ] && ! grep -q 'version.: *.$Versione.' latest.json; then cp latest.json latest.prima.json; fi"
if ($LASTEXITCODE -ne 0) { Fail "Copia del latest.json precedente non riuscita" }
scp -i $ChiaveSsh $latest "${dest}:$Cartella/latest.json"
if ($LASTEXITCODE -ne 0) { Fail "Invio di latest.json non riuscito" }
Write-Host "Pubblicata la versione $Versione su $Server" -ForegroundColor Green
