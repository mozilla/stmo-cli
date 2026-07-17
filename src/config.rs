#![allow(clippy::missing_errors_doc)]

// On macOS, `resolve_api_key`/`login` shell out to `security` instead of linking the
// Security framework directly. Keychain ACLs are keyed to the code signature of the
// process making the Security call, and a child process does not inherit its parent's
// signing identity for that check — so the accessing process is always Apple's
// already-signed `/usr/bin/security`, regardless of stmo-cli's own signature. That means
// stmo-cli needs no Developer ID signing or notarization for Keychain access, on any
// install path (built from source, `cargo install`, or a `cargo binstall`'d release
// tarball), and the one-time "Always Allow" grant (attached to `security`) survives
// stmo-cli rebuilds/upgrades. Linking the framework directly would make stmo-cli itself
// the accessing process, and every unsigned rebuild would present a different identity,
// invalidating the grant and re-prompting.

// Env var wins over the keychain; blank/whitespace values are treated as unset; the
// result is trimmed. Platform-independent, so it's unit-tested directly.
fn pick(env_key: Option<String>, keychain_key: Option<String>) -> Option<String> {
    env_key
        .filter(|k| !k.trim().is_empty())
        .or(keychain_key.filter(|k| !k.trim().is_empty()))
        .map(|k| k.trim().to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn resolve_api_key() -> anyhow::Result<String> {
    pick(std::env::var("REDASH_API_KEY").ok(), None)
        .ok_or_else(|| anyhow::anyhow!("REDASH_API_KEY environment variable not set"))
}

#[cfg(not(target_os = "macos"))]
pub fn login() -> anyhow::Result<()> {
    anyhow::bail!(
        "`stmo-cli login` stores the key in the macOS Keychain and is only available on \
         macOS. On this platform, set the REDASH_API_KEY environment variable instead."
    )
}

#[cfg(target_os = "macos")]
mod macos {
    use super::pick;
    use anyhow::{Context, Result, bail};
    use std::io::IsTerminal;
    use std::process::Command;

    const KEYCHAIN_SERVICE: &str = "stmo-cli";

    // Isolation seam: point `security` at a specific keychain file instead of the
    // default login-keychain search list. Used by the hermetic e2e test in
    // tests/keychain.rs; also usable as an advanced override. Empty/unset => default
    // search list (the user's login keychain).
    fn keychain_file() -> Option<String> {
        std::env::var("STMO_KEYCHAIN_PATH")
            .ok()
            .filter(|p| !p.trim().is_empty())
    }

    // `security` accepts an optional trailing [keychain] positional arg on both
    // find-generic-password and add-generic-password.
    fn with_keychain(base: &[&str]) -> Vec<String> {
        let mut args: Vec<String> = base.iter().map(|s| (*s).to_string()).collect();
        if let Some(path) = keychain_file() {
            args.push(path);
        }
        args
    }

    pub fn resolve_api_key() -> Result<String> {
        if let Some(k) = pick(std::env::var("REDASH_API_KEY").ok(), None) {
            return Ok(k);
        }
        if let Some(k) = pick(None, read_from_keychain()?) {
            return Ok(k);
        }
        if std::io::stderr().is_terminal() {
            store_in_keychain()?;
            if let Some(k) = pick(None, read_from_keychain()?) {
                return Ok(k);
            }
        }
        bail!(
            "REDASH_API_KEY is not set and no '{KEYCHAIN_SERVICE}' key is stored in the \
             macOS Keychain.\nRun `stmo-cli login` in your own terminal once to store it."
        )
    }

    pub fn login() -> Result<()> {
        store_in_keychain()?;
        // Trigger the one-time "Always Allow" dialog now, in the user's terminal.
        read_from_keychain()?;
        println!("Stored the Redash API key in the macOS Keychain (service '{KEYCHAIN_SERVICE}').");
        Ok(())
    }

    fn read_from_keychain() -> Result<Option<String>> {
        let output = Command::new("security")
            .args(with_keychain(&[
                "find-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-w",
            ]))
            .output()
            .context("Failed to run `security` to read the Redash API key from the Keychain")?;
        if !output.status.success() {
            return Ok(None);
        }
        let key = String::from_utf8(output.stdout)
            .context("Keychain item was not valid UTF-8")?
            .trim()
            .to_string();
        Ok((!key.is_empty()).then_some(key))
    }

    fn store_in_keychain() -> Result<()> {
        let account = std::env::var("USER").unwrap_or_else(|_| KEYCHAIN_SERVICE.to_string());
        eprintln!("Enter your Redash API key (https://sql.telemetry.mozilla.org/users/me):");
        let status = Command::new("security")
            .args(with_keychain(&[
                "add-generic-password",
                "-a",
                &account,
                "-s",
                KEYCHAIN_SERVICE,
                "-U",
                "-w",
            ]))
            // Inherit stdio so `security` runs its own hidden prompt on the terminal.
            .status()
            .context("Failed to run `security` to store the Redash API key")?;
        if !status.success() {
            bail!("Failed to store the Redash API key in the macOS Keychain");
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub use macos::{login, resolve_api_key};

#[cfg(test)]
mod tests {
    use super::pick;

    #[test]
    fn env_key_wins_over_keychain() {
        assert_eq!(
            pick(
                Some("env-key".to_string()),
                Some("keychain-key".to_string())
            ),
            Some("env-key".to_string())
        );
    }

    #[test]
    fn blank_env_key_falls_through_to_keychain() {
        assert_eq!(
            pick(Some("   ".to_string()), Some("keychain-key".to_string())),
            Some("keychain-key".to_string())
        );
    }

    #[test]
    fn keychain_used_when_env_unset() {
        assert_eq!(
            pick(None, Some("keychain-key".to_string())),
            Some("keychain-key".to_string())
        );
    }

    #[test]
    fn both_unset_or_blank_returns_none() {
        assert_eq!(pick(None, None), None);
        assert_eq!(pick(Some(String::new()), Some("  ".to_string())), None);
    }

    #[test]
    fn result_is_trimmed() {
        assert_eq!(
            pick(Some("  env-key  ".to_string()), None),
            Some("env-key".to_string())
        );
        assert_eq!(
            pick(None, Some("  keychain-key  ".to_string())),
            Some("keychain-key".to_string())
        );
    }
}
