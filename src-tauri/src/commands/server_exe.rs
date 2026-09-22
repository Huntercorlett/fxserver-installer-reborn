//! Locates the server executable inside an artifact folder.
//!
//! FiveM Legacy ships `FXServer.exe`; FiveM for GTAV Enhanced ships
//! `cfx-server.exe` (see docs.fivem.net "What's Changed in FiveM for GTAV
//! Enhanced").

use std::path::{Path, PathBuf};

pub(crate) const LEGACY_EXECUTABLE: &str = "FXServer.exe";
pub(crate) const ENHANCED_EXECUTABLE: &str = "cfx-server.exe";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerEdition {
    Legacy,
    Enhanced,
}

impl ServerEdition {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ServerEdition::Legacy => "legacy",
            ServerEdition::Enhanced => "enhanced",
        }
    }
}

/// Legacy wins if a folder somehow holds both executables.
pub(crate) fn find_server_executable(dir: &Path) -> Option<(PathBuf, ServerEdition)> {
    [
        (LEGACY_EXECUTABLE, ServerEdition::Legacy),
        (ENHANCED_EXECUTABLE, ServerEdition::Enhanced),
    ]
    .into_iter()
    .map(|(name, edition)| (dir.join(name), edition))
    .find(|(path, _)| path.is_file())
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn is_server_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(LEGACY_EXECUTABLE) || name.eq_ignore_ascii_case(ENHANCED_EXECUTABLE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fxi-exe-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_each_edition_and_prefers_legacy() {
        let dir = temp("detect");
        assert!(find_server_executable(&dir).is_none());

        fs::write(dir.join(ENHANCED_EXECUTABLE), "").unwrap();
        let (path, edition) = find_server_executable(&dir).unwrap();
        assert_eq!(edition, ServerEdition::Enhanced);
        assert!(path.ends_with(ENHANCED_EXECUTABLE));
        assert_eq!(edition.as_str(), "enhanced");

        fs::write(dir.join(LEGACY_EXECUTABLE), "").unwrap();
        assert_eq!(
            find_server_executable(&dir).unwrap().1,
            ServerEdition::Legacy
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn matches_process_names_case_insensitively() {
        assert!(is_server_process_name("fxserver.exe"));
        assert!(is_server_process_name("CFX-SERVER.EXE"));
        assert!(!is_server_process_name("node.exe"));
    }
}
