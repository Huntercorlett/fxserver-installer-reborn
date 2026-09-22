//! Project credits. They are part of the product: the build script refuses to
//! compile without them and the app refuses to start if they are altered.

use serde::Serialize;
use sha2::{Digest, Sha256};

pub struct Credit {
    pub name: &'static str,
    pub discord: &'static str,
}

// CREDITS-BEGIN
pub const CREDITS: [Credit; 2] = [
    Credit { name: "Hunter Corlett", discord: "Huntercorlett" },
    Credit { name: ".zox", discord: "zoxile" },
];
// CREDITS-END

const EXPECTED_DIGEST: &str = "f23f38a3a943883fb3aa0ea85e536ea268fe417cb0656340e76906a220465dec";

#[derive(Serialize)]
pub struct CreditEntry {
    name: &'static str,
    discord: &'static str,
}

fn digest() -> String {
    let mut hasher = Sha256::new();
    for credit in CREDITS.iter() {
        hasher.update(credit.name.as_bytes());
        hasher.update([0u8]);
        hasher.update(credit.discord.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Stops the application when the credits were edited or removed.
pub fn enforce() {
    if digest() != EXPECTED_DIGEST {
        eprintln!("The project credits were modified or removed. They are required and must stay unchanged.");
        std::process::exit(1);
    }
}

#[tauri::command]
pub fn get_credits() -> Vec<CreditEntry> {
    enforce();
    CREDITS
        .iter()
        .map(|credit| CreditEntry {
            name: credit.name,
            discord: credit.discord,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_match_the_expected_digest() {
        assert_eq!(digest(), EXPECTED_DIGEST);
        assert_eq!(CREDITS.len(), 2);
    }
}
