# Legge la rubrica di Outlook classico per importarla in Postamon. Sola lettura:
# in Outlook non cambia niente.
#
# Scrive il risultato in JSON (UTF-8) nel file -Out: le cartelle di contatti di
# ogni casella, con contatti e gruppi, e in closePid il processo di Outlook da
# richiudere se l'ha avviato questo script (Quit() lascia vivo un Outlook
# avviato in background: lo chiude Postamon con Restart Manager).
#
# Uscita 3: Outlook classico non è installato.
param([Parameter(Mandatory = $true)] [string]$Out)
$ErrorActionPreference = 'Stop'

$wasOpen = @(Get-Process OUTLOOK -ErrorAction SilentlyContinue).Count -gt 0
try {
  $ol = New-Object -ComObject Outlook.Application
} catch {
  exit 3
}

function Text($value) {
  if ($null -eq $value) { return '' }
  return ([string]$value).Trim()
}

# Indirizzo SMTP: le voci Exchange ('EX') hanno un indirizzo interno da risolvere.
function Smtp($address, $type) {
  $address = Text $address
  if ($type -ne 'EX' -or $address -match '@') { return $address }
  try {
    $r = $ol.Session.CreateRecipient($address)
    [void]$r.Resolve()
    return Text $r.AddressEntry.GetExchangeUser().PrimarySmtpAddress
  } catch {
    return ''
  }
}

$phoneFields = 'MobileTelephoneNumber', 'BusinessTelephoneNumber', 'HomeTelephoneNumber',
  'Business2TelephoneNumber', 'PrimaryTelephoneNumber', 'OtherTelephoneNumber'

function Read-Folder($folder, $store) {
  $contacts = New-Object System.Collections.ArrayList
  $groups = New-Object System.Collections.ArrayList
  foreach ($it in $folder.Items) {
    try {
      if ($it.Class -eq 40) {
        # olContact
        $emails = @()
        foreach ($n in 1..3) {
          $address = Smtp $it."Email${n}Address" $it."Email${n}AddressType"
          if ($address) { $emails += [ordered]@{ address = $address; display = Text $it."Email${n}DisplayName" } }
        }
        $phones = @()
        foreach ($field in $phoneFields) {
          $phone = Text $it.$field
          if ($phone) { $phones += $phone }
        }
        [void]$contacts.Add([ordered]@{
            fullName = Text $it.FullName
            fileAs   = Text $it.FileAs
            company  = Text $it.CompanyName
            emails   = $emails
            phones   = $phones
            notes    = Text $it.Body
          })
      } elseif ($it.Class -eq 69) {
        # olDistributionList
        $members = @()
        for ($i = 1; $i -le $it.MemberCount; $i++) {
          $m = $it.GetMember($i)
          $members += [ordered]@{ name = Text $m.Name; address = Smtp $m.Address $m.AddressEntry.Type }
        }
        [void]$groups.Add([ordered]@{ name = Text $it.DLName; members = $members })
      }
    } catch {
      # una voce illeggibile non ferma le altre
    }
  }
  return [ordered]@{ store = $store; name = Text $folder.Name; contacts = $contacts.ToArray(); groups = $groups.ToArray() }
}

$folders = New-Object System.Collections.ArrayList

function Walk($folder, $store) {
  if ($folder.DefaultItemType -eq 2) { [void]$folders.Add((Read-Folder $folder $store)) }
  foreach ($sub in $folder.Folders) { Walk $sub $store }
}

$result = [ordered]@{ folders = @(); closePid = 0; error = '' }
try {
  foreach ($store in $ol.GetNamespace('MAPI').Stores) {
    try { Walk ($store.GetRootFolder()) (Text $store.DisplayName) } catch { }
  }
  $result.folders = $folders.ToArray()
} catch {
  $result.error = $_.Exception.Message
}
# Outlook avviato da qui e ancora senza finestre: va richiuso.
try {
  if (-not $wasOpen -and $ol.Explorers.Count -eq 0 -and $ol.Inspectors.Count -eq 0) {
    $p = Get-Process OUTLOOK -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($p) { $result.closePid = $p.Id }
  }
} catch { }
$json = ConvertTo-Json -InputObject $result -Depth 8 -Compress
[IO.File]::WriteAllText($Out, $json, (New-Object System.Text.UTF8Encoding $false))
