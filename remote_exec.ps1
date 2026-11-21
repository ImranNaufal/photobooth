param([string]$Script)
$bytes = [System.Text.Encoding]::Unicode.GetBytes($Script)
$b64 = [Convert]::ToBase64String($bytes)
ssh Administrator@100.64.228.69 "powershell -NoProfile -EncodedCommand $b64"
