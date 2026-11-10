//! Encrypted secrets vault.
//!
//! File layout (`vault.bin`):
//! ```text
//! magic "SPV1" | m_cost u32 | t_cost u32 | p_cost u32 | salt [16] | nonce [24] | ciphertext
//! ```
//! The key is derived from the master password with Argon2id and the payload
//! (JSON list of secrets) is sealed with XChaCha20-Poly1305. The whole header
//! is authenticated as associated data, so tampering with KDF params fails.

pub mod keyring;

use anyhow::{anyhow, bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const MAGIC: &[u8; 4] = b"SPV1";
const HEADER_LEN: usize = 4 + 12 + 16 + 24;

#[derive(Clone, Copy, Debug)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// Upper bounds accepted from a vault file header. The header is read
    /// before anything is authenticated, so a tampered file must not be able to
    /// demand terabytes of memory or hours of work.
    const MAX_M_COST: u32 = 1024 * 1024; // KiB = 1 GiB
    const MAX_T_COST: u32 = 20;
    const MAX_P_COST: u32 = 16;

    fn check(&self) -> Result<()> {
        if self.m_cost == 0 || self.m_cost > Self::MAX_M_COST || self.t_cost == 0 || self.t_cost > Self::MAX_T_COST
            || self.p_cost == 0 || self.p_cost > Self::MAX_P_COST
        {
            bail!("vault file has unreasonable key-derivation parameters (corrupted or tampered)");
        }
        Ok(())
    }
}

impl Default for KdfParams {
    fn default() -> Self {
        // 64 MiB, 3 passes, 1 lane: OWASP-recommended Argon2id baseline.
        Self { m_cost: 64 * 1024, t_cost: 3, p_cost: 1 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Password,
    PrivateKey,
    Token,
    Note,
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Secret {
    #[zeroize(skip)]
    pub id: String,
    #[zeroize(skip)]
    pub name: String,
    #[zeroize(skip)]
    pub kind: SecretKind,
    /// Username hint (optional, not used for auth unless host has none).
    #[serde(default)]
    #[zeroize(skip)]
    pub username: Option<String>,
    /// The secret itself: password, PEM/OpenSSH private key, token, note text.
    pub value: String,
    /// Passphrase for an encrypted private key.
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    #[zeroize(skip)]
    pub notes: String,
    #[zeroize(skip)]
    pub updated_at: u64,
}

/// What the UI is allowed to see about a secret without revealing it.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SecretMeta {
    pub id: String,
    pub name: String,
    pub kind: SecretKind,
    pub username: Option<String>,
    pub notes: String,
    pub has_passphrase: bool,
    pub updated_at: u64,
}

impl From<&Secret> for SecretMeta {
    fn from(s: &Secret) -> Self {
        SecretMeta {
            id: s.id.clone(),
            name: s.name.clone(),
            kind: s.kind,
            username: s.username.clone(),
            notes: s.notes.clone(),
            has_passphrase: s.passphrase.as_deref().is_some_and(|p| !p.is_empty()),
            updated_at: s.updated_at,
        }
    }
}

/// Input from the UI when creating/updating a secret. `None` value on update
/// means "keep the existing value".
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: SecretKind,
    pub username: Option<String>,
    pub value: Option<String>,
    pub passphrase: Option<String>,
    #[serde(default)]
    pub notes: String,
}

impl std::fmt::Debug for SecretInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretInput")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("value", &self.value.as_ref().map(|_| "<redacted>"))
            .field("passphrase", &self.passphrase.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

#[derive(Default, Serialize, Deserialize)]
struct VaultData {
    secrets: Vec<Secret>,
}

pub struct Vault {
    path: PathBuf,
    key: Option<Zeroizing<[u8; 32]>>,
    salt: [u8; 16],
    params: KdfParams,
    data: Option<VaultData>,
    pub last_activity: Instant,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub remembered: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn derive_key(password: &str, salt: &[u8; 16], p: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(p.m_cost, p.t_cost, p.p_cost, Some(32))
        .map_err(|e| anyhow!("argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow!("argon2: {e}"))?;
    Ok(key)
}

fn header(params: KdfParams, salt: &[u8; 16], nonce: &[u8; 24]) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&params.m_cost.to_le_bytes());
    h.extend_from_slice(&params.t_cost.to_le_bytes());
    h.extend_from_slice(&params.p_cost.to_le_bytes());
    h.extend_from_slice(salt);
    h.extend_from_slice(nonce);
    h
}

struct ParsedFile<'a> {
    params: KdfParams,
    salt: [u8; 16],
    nonce: [u8; 24],
    header: &'a [u8],
    ciphertext: &'a [u8],
}

fn parse_file(bytes: &[u8]) -> Result<ParsedFile<'_>> {
    if bytes.len() < HEADER_LEN + 16 || &bytes[..4] != MAGIC {
        bail!("not a Shellport vault file");
    }
    let u = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let params = KdfParams { m_cost: u(4), t_cost: u(8), p_cost: u(12) };
    params.check()?;
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&bytes[16..32]);
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&bytes[32..56]);
    Ok(ParsedFile { params, salt, nonce, header: &bytes[..HEADER_LEN], ciphertext: &bytes[HEADER_LEN..] })
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Vault {
            path,
            key: None,
            salt: [0; 16],
            params: KdfParams::default(),
            data: None,
            last_activity: Instant::now(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn is_unlocked(&self) -> bool {
        self.data.is_some()
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Create a brand-new empty vault. Fails if one already exists.
    pub fn create(&mut self, password: &str, params: KdfParams) -> Result<()> {
        if self.exists() {
            bail!("a vault already exists");
        }
        if password.chars().count() < 8 {
            bail!("master password must be at least 8 characters");
        }
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        self.key = Some(derive_key(password, &salt, params)?);
        self.salt = salt;
        self.params = params;
        self.data = Some(VaultData::default());
        self.save()?;
        self.touch();
        Ok(())
    }

    /// Unlock with the master password. Returns the derived key so callers can
    /// optionally persist it in the system keyring.
    pub fn unlock(&mut self, password: &str) -> Result<Zeroizing<[u8; 32]>> {
        let bytes = std::fs::read(&self.path).context("reading vault")?;
        let f = parse_file(&bytes)?;
        let key = derive_key(password, &f.salt, f.params)?;
        self.open_with_key(&bytes, key.clone())?;
        Ok(key)
    }

    /// Unlock with a previously derived key (from the system keyring).
    pub fn unlock_with_key(&mut self, key: Zeroizing<[u8; 32]>) -> Result<()> {
        let bytes = std::fs::read(&self.path).context("reading vault")?;
        self.open_with_key(&bytes, key)
    }

    fn open_with_key(&mut self, bytes: &[u8], key: Zeroizing<[u8; 32]>) -> Result<()> {
        let f = parse_file(bytes)?;
        let cipher = XChaCha20Poly1305::new(key.as_ref().into());
        let plain = Zeroizing::new(
            cipher
                .decrypt(XNonce::from_slice(&f.nonce), Payload { msg: f.ciphertext, aad: f.header })
                .map_err(|_| anyhow!("wrong master password or corrupted vault"))?,
        );
        let data: VaultData = serde_json::from_slice(&plain).context("decoding vault")?;
        self.key = Some(key);
        self.salt = f.salt;
        self.params = f.params;
        self.data = Some(data);
        self.touch();
        Ok(())
    }

    pub fn lock(&mut self) {
        self.key = None;
        self.data = None;
    }

    fn save(&self) -> Result<()> {
        let key = self.key.as_ref().ok_or_else(|| anyhow!("vault is locked"))?;
        let data = self.data.as_ref().ok_or_else(|| anyhow!("vault is locked"))?;
        let plain = to_json_zeroizing(data)?;
        let mut nonce = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut nonce);
        let hdr = header(self.params, &self.salt, &nonce);
        let cipher = XChaCha20Poly1305::new(key.as_ref().into());
        let ct = cipher
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &plain, aad: &hdr })
            .map_err(|_| anyhow!("encryption failed"))?;
        let mut out = hdr;
        out.extend_from_slice(&ct);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::paths::write_private(&self.path, &out)?;
        Ok(())
    }

    fn data(&self) -> Result<&VaultData> {
        self.data.as_ref().ok_or_else(|| anyhow!("vault is locked"))
    }

    fn data_mut(&mut self) -> Result<&mut VaultData> {
        self.data.as_mut().ok_or_else(|| anyhow!("vault is locked"))
    }

    pub fn list(&self) -> Result<Vec<SecretMeta>> {
        let mut v: Vec<SecretMeta> = self.data()?.secrets.iter().map(SecretMeta::from).collect();
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(v)
    }

    pub fn get(&self, id: &str) -> Result<Secret> {
        self.data()?
            .secrets
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("secret not found (is the vault unlocked?)"))
    }

    pub fn upsert(&mut self, input: SecretInput) -> Result<SecretMeta> {
        if input.name.trim().is_empty() {
            bail!("name is required");
        }
        if input.kind == SecretKind::PrivateKey {
            if let Some(v) = input.value.as_deref() {
                russh::keys::decode_secret_key(v, input.passphrase.as_deref().filter(|p| !p.is_empty()))
                    .map_err(|e| anyhow!("private key could not be parsed: {e}"))?;
            }
        }
        let data = self.data_mut()?;
        let meta = match input.id.as_deref().and_then(|id| data.secrets.iter_mut().find(|s| s.id == id)) {
            Some(s) => {
                s.name = input.name;
                s.kind = input.kind;
                s.username = input.username.filter(|u| !u.is_empty());
                if let Some(v) = input.value {
                    s.value = v;
                }
                if let Some(p) = input.passphrase {
                    s.passphrase = if p.is_empty() { None } else { Some(p) };
                }
                s.notes = input.notes;
                s.updated_at = now();
                SecretMeta::from(&*s)
            }
            None => {
                let s = Secret {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: input.name,
                    kind: input.kind,
                    username: input.username.filter(|u| !u.is_empty()),
                    value: input.value.unwrap_or_default(),
                    passphrase: input.passphrase.filter(|p| !p.is_empty()),
                    notes: input.notes,
                    updated_at: now(),
                };
                let m = SecretMeta::from(&s);
                data.secrets.push(s);
                m
            }
        };
        self.save()?;
        self.touch();
        Ok(meta)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let data = self.data_mut()?;
        let before = data.secrets.len();
        data.secrets.retain(|s| s.id != id);
        if data.secrets.len() == before {
            bail!("secret not found");
        }
        self.save()?;
        self.touch();
        Ok(())
    }

    /// Re-encrypt under a new master password (new salt).
    pub fn change_password(&mut self, old: &str, new: &str) -> Result<Zeroizing<[u8; 32]>> {
        if new.chars().count() < 8 {
            bail!("master password must be at least 8 characters");
        }
        let bytes = std::fs::read(&self.path)?;
        let f = parse_file(&bytes)?;
        let old_key = derive_key(old, &f.salt, f.params)?;
        if self.key.as_deref() != Some(&*old_key) {
            bail!("current master password is incorrect");
        }
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let params = KdfParams::default();
        let key = derive_key(new, &salt, params)?;
        self.key = Some(key.clone());
        self.salt = salt;
        self.params = params;
        self.save()?;
        Ok(key)
    }

    /// Export a public key for a stored private key (for `authorized_keys`).
    pub fn public_key_openssh(&self, id: &str) -> Result<String> {
        let s = self.get(id)?;
        if s.kind != SecretKind::PrivateKey {
            bail!("secret is not a private key");
        }
        let k = russh::keys::decode_secret_key(&s.value, s.passphrase.as_deref())?;
        Ok(k.public_key().to_openssh()?)
    }
}

/// Serialize into a buffer sized exactly up front, so no partially-filled
/// copies of the plaintext are freed (unwiped) while the Vec grows.
fn to_json_zeroizing<T: Serialize>(v: &T) -> Result<Zeroizing<Vec<u8>>> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut c = Count(0);
    serde_json::to_writer(&mut c, v)?;
    let mut out = Zeroizing::new(Vec::with_capacity(c.0));
    serde_json::to_writer(&mut *out, v)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast() -> KdfParams {
        KdfParams { m_cost: 1024, t_cost: 1, p_cost: 1 }
    }

    fn input(name: &str, value: &str) -> SecretInput {
        SecretInput {
            id: None,
            name: name.into(),
            kind: SecretKind::Password,
            username: Some("root".into()),
            value: Some(value.into()),
            passphrase: None,
            notes: String::new(),
        }
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let mut v = Vault::new(path.clone());
        v.create("correct horse", fast()).unwrap();
        let m = v.upsert(input("prod db", "hunter2")).unwrap();
        v.lock();
        assert!(v.list().is_err());

        let mut v2 = Vault::new(path.clone());
        assert!(v2.unlock("wrong password").is_err());
        let key = v2.unlock("correct horse").unwrap();
        assert_eq!(v2.get(&m.id).unwrap().value, "hunter2");

        let mut v3 = Vault::new(path);
        v3.unlock_with_key(key).unwrap();
        assert_eq!(v3.list().unwrap().len(), 1);
    }

    #[test]
    fn tamper_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let mut v = Vault::new(path.clone());
        v.create("correct horse", fast()).unwrap();
        v.upsert(input("x", "y")).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(Vault::new(path.clone()).unlock("correct horse").is_err());
        // Tampering with the header (KDF params) is also caught via AAD.
        bytes[last] ^= 1;
        bytes[8] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(Vault::new(path).unlock("correct horse").is_err());
    }

    #[test]
    fn absurd_kdf_params_rejected_before_derivation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let mut v = Vault::new(path.clone());
        v.create("correct horse", fast()).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes()); // m_cost ≈ 4 TiB
        std::fs::write(&path, &bytes).unwrap();
        let t = std::time::Instant::now();
        let err = Vault::new(path).unlock("correct horse").unwrap_err();
        assert!(format!("{err:#}").contains("key-derivation parameters"), "{err:#}");
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn secret_input_debug_is_redacted() {
        let i = input("db", "hunter2");
        assert!(!format!("{i:?}").contains("hunter2"));
    }

    #[test]
    fn update_keeps_value_and_change_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.bin");
        let mut v = Vault::new(path.clone());
        v.create("correct horse", fast()).unwrap();
        let m = v.upsert(input("a", "secret")).unwrap();
        let mut upd = input("renamed", "");
        upd.id = Some(m.id.clone());
        upd.value = None;
        v.upsert(upd).unwrap();
        assert_eq!(v.get(&m.id).unwrap().value, "secret");
        assert_eq!(v.get(&m.id).unwrap().name, "renamed");
        assert!(v.change_password("nope nope", "new password!").is_err());
        v.change_password("correct horse", "new password!").unwrap();
        assert!(Vault::new(path.clone()).unlock("correct horse").is_err());
        Vault::new(path).unlock("new password!").unwrap();
    }
}
