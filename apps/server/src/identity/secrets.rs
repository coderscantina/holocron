//! Encryption of second-factor secrets at rest (decision 0012): XChaCha20-Poly1305 under a
//! 32-byte key in `<data-dir>/secrets.key`, never stored in `control.sqlite`. The user ID is
//! associated data, so a ciphertext copied to another account does not open.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use holocron_protocol::ids::UserId;

pub const SECRETS_KEY_FILE: &str = "secrets.key";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;

/// The installation's secret key. Deliberately without `Debug`.
pub struct SecretKey(XChaCha20Poly1305);

impl SecretKey {
    /// Reads `secrets.key` from the data directory, creating it (mode 0600) when absent.
    pub fn load_or_create(dir: &Path) -> io::Result<Self> {
        let path = dir.join(SECRETS_KEY_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => create(&path)?,
            Err(e) => return Err(e),
        };
        let cipher = XChaCha20Poly1305::new_from_slice(&bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} must hold {KEY_BYTES} bytes", path.display()),
            )
        })?;
        Ok(Self(cipher))
    }

    /// A random nonce followed by the ciphertext and tag.
    pub fn seal(&self, user: &UserId, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).expect("the OS provides randomness");
        let payload = Payload {
            msg: plain,
            aad: user.as_str().as_bytes(),
        };
        let sealed = self
            .0
            .encrypt(&XNonce::from(nonce), payload)
            .expect("encryption of a short secret cannot fail");
        [nonce.as_slice(), &sealed].concat()
    }

    /// `None` when the ciphertext belongs to another user or another key.
    pub fn open(&self, user: &UserId, sealed: &[u8]) -> Option<Vec<u8>> {
        let (nonce, ciphertext) = sealed.split_at_checked(NONCE_BYTES)?;
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().ok()?;
        let payload = Payload {
            msg: ciphertext,
            aad: user.as_str().as_bytes(),
        };
        self.0.decrypt(&XNonce::from(nonce), payload).ok()
    }
}

/// Writes a new random key. `create_new` makes a concurrent first start read the winner's key.
fn create(path: &Path) -> io::Result<Vec<u8>> {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key).expect("the OS provides randomness");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&key)?;
            file.sync_all()?;
            Ok(key.to_vec())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => fs::read(path),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertexts_open_only_for_their_user_and_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = SecretKey::load_or_create(dir.path()).unwrap();
        let alice = UserId::new("alice");
        let sealed = key.seal(&alice, b"secret");
        assert_eq!(key.open(&alice, &sealed).unwrap(), b"secret");
        assert!(key.open(&UserId::new("bob"), &sealed).is_none());

        let again = SecretKey::load_or_create(dir.path()).unwrap();
        assert_eq!(again.open(&alice, &sealed).unwrap(), b"secret");
        let other = tempfile::TempDir::new().unwrap();
        let foreign = SecretKey::load_or_create(other.path()).unwrap();
        assert!(foreign.open(&alice, &sealed).is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(SECRETS_KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
