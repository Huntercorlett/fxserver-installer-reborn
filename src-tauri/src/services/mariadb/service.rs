use std::process::Command;
use std::time::Duration;

use crate::process::CommandNoWindowExt;
use crate::services::mariadb::detect::find_service_name;
use crate::services::mariadb::install::run_elevated_powershell_script;

const ELEVATED_SERVICE_TIMEOUT: Duration = Duration::from_secs(150);

pub fn start_service(service_name: Option<String>) -> Result<(), String> {
    control_service("start", service_name)
}

pub fn stop_service(service_name: Option<String>) -> Result<(), String> {
    control_service("stop", service_name)
}

pub fn restart_service(service_name: Option<String>) -> Result<(), String> {
    let service_name = service_name
        .or_else(find_service_name)
        .ok_or_else(missing_service_message)?;
    match run_sc("stop", &service_name) {
        // Not allowed to control the service: do the whole restart in a single
        // elevated step so Windows only asks for approval once.
        Err(error) if is_access_denied(&error) => run_elevated("restart", &service_name),
        // Any other stop failure (for example "not running") is fine; start decides.
        _ => run_service_action("start", &service_name),
    }
}

fn control_service(action: &str, service_name: Option<String>) -> Result<(), String> {
    let service_name = service_name
        .or_else(find_service_name)
        .ok_or_else(missing_service_message)?;
    run_service_action(action, &service_name)
}

/// Runs `sc`, and if Windows denies access (the app is not elevated) retries
/// through the app's UAC helper.
fn run_service_action(action: &str, service_name: &str) -> Result<(), String> {
    match run_sc(action, service_name) {
        Err(error) if is_access_denied(&error) => run_elevated(action, service_name),
        other => other,
    }
}

fn run_sc(action: &str, service_name: &str) -> Result<(), String> {
    let output = Command::new("sc")
        .no_window()
        .args([action, service_name])
        .output()
        .map_err(|error| format!("Failed to run service command: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn is_access_denied(message: &str) -> bool {
    message.contains("FAILED 5") || message.to_ascii_lowercase().contains("access is denied")
}

fn run_elevated(action: &str, service_name: &str) -> Result<(), String> {
    let script = elevated_service_script(action, service_name)?;
    let output = run_elevated_powershell_script(
        "mariadb-service-control",
        &script,
        ELEVATED_SERVICE_TIMEOUT,
    )?;
    if output.success {
        return Ok(());
    }
    let detail = output.stderr.trim();
    Err(if detail.is_empty() {
        "The administrator approval prompt was declined or the service command failed.".to_string()
    } else {
        detail.to_string()
    })
}

fn elevated_service_script(action: &str, service_name: &str) -> Result<String, String> {
    if !matches!(action, "start" | "stop" | "restart") {
        return Err("Unsupported service action.".to_string());
    }
    if service_name.trim().is_empty() || service_name.chars().any(char::is_control) {
        return Err("The MariaDB service name is invalid.".to_string());
    }
    let service_name = service_name.replace('\'', "''");
    Ok(format!(
        r#"$ErrorActionPreference = 'Stop'
$serviceName = '{service_name}'
$action = '{action}'
$service = Get-Service -Name $serviceName
if ($action -eq 'restart') {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(60))
    }}
    Start-Service -Name $serviceName
    $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(60))
}} elseif ($action -eq 'start') {{
    if ($service.Status -ne 'Running') {{
        Start-Service -Name $serviceName
        $service.WaitForStatus('Running', [TimeSpan]::FromSeconds(60))
    }}
}} else {{
    if ($service.Status -ne 'Stopped') {{
        Stop-Service -Name $serviceName -Force
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(60))
    }}
}}
exit 0
"#
    ))
}

fn missing_service_message() -> String {
    "No MariaDB Windows service was found.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_access_denied_output() {
        assert!(is_access_denied(
            "[SC] StartService: OpenService FAILED 5:\n\nAccess is denied."
        ));
        assert!(is_access_denied("access is denied"));
        assert!(!is_access_denied(
            "[SC] StartService FAILED 1056: An instance of the service is already running."
        ));
    }

    #[test]
    fn builds_a_quoted_script_and_rejects_bad_input() {
        let script = elevated_service_script("start", "Maria'DB").unwrap();
        assert!(script.contains("$serviceName = 'Maria''DB'"));
        assert!(script.contains("$action = 'start'"));
        assert!(elevated_service_script("delete", "MariaDB").is_err());
        assert!(elevated_service_script("start", "  ").is_err());
        assert!(elevated_service_script("start", "a\nb").is_err());
    }
}
