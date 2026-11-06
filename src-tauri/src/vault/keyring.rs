//! Optional "remember on this device": stores the derived vault key in the
//! desktop Secret Service (GNOME Keyring / KWallet) so the vault can unlock
//! without prompting for the master password.

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use zeroize::Zeroizing;

const SERVICE: &str = "shellport";
const ACCOUNT: &str = "vault-key";

fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, ACCOUNT)?)
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
