//! Optional "remember on this device": stores the derived vault key in the
//! desktop Secret Service (GNOME Keyring / KWallet) so the vault can unlock
//! without prompting for the master password.

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use zeroize::Zeroizing;

const SERVICE: &str = "shellport";
const ACCOUNT: &str = "vault-key";

/// Keyring account for the vault in `config_dir`. The standard profile keeps
/// the original name; any other profile (`$SHELLPORT_CONFIG_DIR`) gets its own
/// entry so it can never overwrite or delete the main vault's key.
fn account_for(config_dir: &std::path::Path, default_dir: &std::path::Path) -> String {
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dir = canon(config_dir);
    if dir == canon(default_dir) {
        return ACCOUNT.to_string();
    }
    use sha1::Digest;
    let digest = sha1::Sha1::digest(dir.to_string_lossy().as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("{ACCOUNT}:{hex}")
}

fn entry() -> Result<keyring::Entry> {
    let account = account_for(
        &crate::paths::config_dir(),
        &crate::paths::default_config_dir(),
    );
    Ok(keyring::Entry::new(SERVICE, &account)?)
}

pub fn store(key: &[u8; 32]) -> Result<()> {
    let encoded = Zeroizing::new(STANDARD.encode(key));
    entry()?.set_password(&encoded)?;
    Ok(())
}

pub fn load() -> Result<Option<Zeroizing<[u8; 32]>>> {
    match entry()?.get_password() {
        Ok(s) => {
            let s = Zeroizing::new(s);
            let bytes = Zeroizing::new(STANDARD.decode(s.as_bytes())?);
            if bytes.len() != 32 {
                return Err(anyhow!("keyring entry has wrong length"));
            }
            let mut key = Zeroizing::new([0u8; 32]);
            key.copy_from_slice(&bytes);
            Ok(Some(key))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn clear() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn is_stored() -> bool {
    matches!(load(), Ok(Some(_)))
}

#[cfg(test)]
mod tests {
    use super::account_for;

    #[test]
    fn default_profile_keeps_original_account_others_differ() {
        let d = tempfile::tempdir().unwrap();
        let (main, other, other2) = (
            d.path().join("main"),
            d.path().join("demo"),
            d.path().join("demo2"),
        );
        for p in [&main, &other, &other2] {
            std::fs::create_dir(p).unwrap();
        }
        assert_eq!(account_for(&main, &main), "vault-key");
        let a = account_for(&other, &main);
        assert!(
            a.starts_with("vault-key:") && a.len() == "vault-key:".len() + 16,
            "{a}"
        );
        assert_ne!(a, account_for(&other2, &main));
        assert_eq!(a, account_for(&other, &main), "stable");
    }
}
