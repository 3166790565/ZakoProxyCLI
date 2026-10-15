param([string]$Archive = 'D:\Download\gpt-instruct-main.zip')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
$models = @('gpt-5.6-sol', 'gpt-6-astra', 'gpt-6.1-sol')
$packages = @('gpt-5.6-sol-v45.zip', 'gpt-6-astra-v2-rc1.zip', 'gpt-6.1-sol-v1-rc2.zip')
$result = @()
try {
  for ($i = 0; $i -lt $packages.Count; $i++) {
    $entry = $zip.GetEntry('gpt-instruct-main/' + $packages[$i])
    $memory = [IO.MemoryStream]::new()
    $stream = $entry.Open()
    try { $stream.CopyTo($memory) } finally { $stream.Dispose() }
    $memory.Position = 0
    $inner = [IO.Compression.ZipArchive]::new($memory, [IO.Compression.ZipArchiveMode]::Read)
    try {
      $markdown = @($inner.Entries | Where-Object FullName -like '*.md')[0]
      $reader = [IO.StreamReader]::new($markdown.Open())
      try { $instruction = $reader.ReadToEnd() } finally { $reader.Dispose() }
      $result += @{ id = $packages[$i].Replace('.zip', ''); name = $packages[$i].Replace('.zip', ''); models = @($models[$i]); instruction = $instruction }
    } finally { $inner.Dispose(); $memory.Dispose() }
  }
  $license = $zip.GetEntry('gpt-instruct-main/LICENSE')
  $reader = [IO.StreamReader]::new($license.Open())
  try { $text = $reader.ReadToEnd() } finally { $reader.Dispose() }
  [IO.Directory]::CreateDirectory((Join-Path $root 'third_party')) | Out-Null
  [IO.File]::WriteAllText((Join-Path $root 'third_party/gpt-instruct.LICENSE'), $text, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText((Join-Path $root 'profiles/index.json'), (ConvertTo-Json -InputObject $result -Depth 8), [Text.UTF8Encoding]::new($false))
  Write-Output "Imported $($result.Count) upstream profiles as inert request data."
} finally { $zip.Dispose() }
