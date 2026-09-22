$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$api = 'https://downloads.mariadb.org/rest-api/mariadb'

try {
    if ($series) {
        $metadata = Invoke-RestMethod -Uri "$api/$series/" -TimeoutSec 30
        ConvertTo-Json -InputObject @($metadata.releases.PSObject.Properties.Value |
            Where-Object { $_.release_id -match '^\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.release_id } -Descending |
            ForEach-Object { [pscustomobject]@{ version = $_.release_id; date = $_.date_of_release } }) -Compress
    } else {
        $index = Invoke-RestMethod -Uri "$api/" -TimeoutSec 30
        ConvertTo-Json -InputObject @($index.major_releases |
            Where-Object { $_.release_id -match '^\d+\.\d+$' -and $_.release_status -notmatch 'Alpha|Beta|RC|Preview|Development' } |
            Sort-Object { [version]$_.release_id } -Descending |
            ForEach-Object { [pscustomobject]@{ series = $_.release_id; status = $_.release_status; support = $_.release_support_type; eol = $_.release_eol_date } }) -Compress
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
