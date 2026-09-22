$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$api = 'https://downloads.mariadb.org/rest-api/mariadb'

try {
    if ($requested -match '^\d+\.\d+\.\d+$') {
        $metadata = Invoke-RestMethod -Uri "$api/$requested/" -TimeoutSec 30
    } else {
        $seriesId = $requested
        if (-not $seriesId) {
            # FiveM/txAdmin/oxmysql and XAMPP-era dumps expect the 10.x behaviour (native password login,
            # utf8mb4_general_ci default), so the default is the 10.11 LTS series.
            $index = Invoke-RestMethod -Uri "$api/" -TimeoutSec 30
            $seriesId = '10.11'
            if (-not ($index.major_releases | Where-Object { $_.release_id -eq $seriesId })) {
                $seriesId = ($index.major_releases |
                    Where-Object { $_.release_status -eq 'Stable' -and $_.release_id -match '^\d+\.\d+$' } |
                    Sort-Object { [version]$_.release_id } -Descending |
                    Select-Object -First 1).release_id
            }
            if (-not $seriesId) { throw 'No stable MariaDB release was found.' }
        }
        $metadata = Invoke-RestMethod -Uri "$api/$seriesId/latest/" -TimeoutSec 30
    }
    $release = $metadata.releases.PSObject.Properties.Value | Select-Object -First 1
    $file = $release.files |
        Where-Object { $_.os -eq 'Windows' -and $_.cpu -eq 'x86_64' -and $_.file_name -match '-winx64\.msi$' } |
        Select-Object -First 1
    if (-not $file) { throw 'No Windows x64 MSI was published for this MariaDB release.' }

    [pscustomobject]@{
        version = $release.release_id
        file_name = $file.file_name
        sha256 = $file.checksum.sha256sum
    } | ConvertTo-Json -Compress
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
