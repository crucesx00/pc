use crate::error::{Error, Result};
use crate::keys::{gen_box_keypair, PublicKey, SecretKey};

use base64;
use rmp_serde::{decode, encode};
use serde_derive::{Deserialize, Serialize};
use sodiumoxide::crypto::box_;
use sodiumoxide::crypto::hash::sha256::hash;
use sodiumoxide::crypto::pwhash::{self, Salt, SALTBYTES};
use sodiumoxide::crypto::sealedbox;
use sodiumoxide::crypto::secretbox;
use sodiumoxide::randombytes;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Identifier([u8; 8]);

impl Identifier {
    pub fn new() -> Identifier {
        let mut id = Identifier([0; 8]);
        randombytes::randombytes_into(&mut id.0);

        id
    }

    pub fn to_string(&self) -> String {
        format!("{}", self)
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strings: Vec<String> = self.0.iter().map(|b| format!("{:02x}", b)).collect();
        write!(f, "{}", strings.join(":"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Identity {
    pub name: String,
    pub identifier: Identifier,
    fingerprint: Fingerprint,
    public_key: PublicKey,
    private_key: SecretKey,
}

impl Identity {
    pub fn new(name: &str) -> Self {
        let (public_key, private_key) = gen_box_keypair();
        Self {
            name: name.into(),
            identifier: Identifier::new(),
            fingerprint: Fingerprint::new(&public_key),
            public_key,
            private_key,
        }
    }

    pub fn public_identity(&self) -> PublicIdentity {
        PublicIdentity {
            name: self.name.clone(),
            identifier: self.identifier.clone(),
            fingerprint: self.fingerprint.clone(),
            public_key: self.public_key.clone(),
        }
    }

    pub fn decrypt(&self, msg: &[u8], nonce: &box_::Nonce, pk: PublicKey) -> Result<Vec<u8>> {
        match box_::open(msg, nonce, &pk.into(), &self.private_key.clone().into()) {
            Ok(msg) => Ok(msg),
            Err(_) => Err(Error::Error("Error decrypting message".into())),
        }
    }

    pub fn decrypt_anonymous(&self, msg: &[u8]) -> Result<Vec<u8>> {
        match sealedbox::open(
            msg,
            &self.public_key.clone().into(),
            &self.private_key.clone().into(),
        ) {
            Ok(msg) => Ok(msg),
            Err(_) => Err(Error::Error("Error decrypting message".into())),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PublicIdentity {
    name: String,
    pub identifier: Identifier,
    fingerprint: Fingerprint,
    public_key: PublicKey,
}

impl fmt::Display for PublicIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.identifier)
    }
}

impl PublicIdentity {
    pub fn new() -> Self {
        Self {
            name: String::default(),
            identifier: Identifier::new(),
            fingerprint: Fingerprint::default(),
            public_key: PublicKey::default(),
        }
    }

    fn serialize(&self) -> Result<Vec<u8>> {
        Ok(encode::to_vec(self)?)
    }

    pub fn encrypt(&self, msg: &[u8], nonce: &box_::Nonce, sk: SecretKey) -> Vec<u8> {
        box_::seal(msg, nonce, &self.public_key.clone().into(), &sk.into())
    }

    pub fn encrypt_anonymous(&self, msg: &[u8]) -> Vec<u8> {
        sealedbox::seal(msg, &self.public_key.clone().into())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct Fingerprint(Vec<u8>);

impl Fingerprint {
    fn new(key: &PublicKey) -> Self {
        Self(hash(key.as_ref()).as_ref().to_vec())
    }
}

impl From<Fingerprint> for String {
    fn from(fingerprint: Fingerprint) -> String {
        let strings: Vec<String> = fingerprint.0.iter().map(|b| format!("{:02x}", b)).collect();
        strings.join(":")
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strings: Vec<String> = self.0.iter().map(|b| format!("{:2x}", b)).collect();
        write!(f, "{}", strings.join(":"))
    }
}

#[derive(Debug)]
pub struct IdentityFile<F: Read + Write + Seek> {
    file: F,
    password: String,
    salt: Salt,
    nonce: secretbox::Nonce,
    pub identities: Vec<Identity>,
    pub trusted: Arc<Mutex<Vec<PublicIdentity>>>,
}

impl IdentityFile<File> {
    pub async fn open(path: &Path, password: String) -> Result<Self> {
        if path.exists() {
            let mut file = OpenOptions::new().read(true).write(true).open(path)?;
            let salt = IdentityFile::<File>::read_salt(&mut file)?;
            let nonce = IdentityFile::<File>::read_nonce(&mut file)?;
            let key = IdentityFile::<File>::derive_key(&password, salt)?;
            let data = IdentityFile::<File>::decrypt_identity_data(&mut file, nonce, key)?;
            let identities: Vec<Identity> = decode::from_read_ref(&data)?;
            let trusted: Arc<Mutex<Vec<PublicIdentity>>> =
                Arc::new(Mutex::new(decode::from_read_ref(&data)?));
            Ok(Self {
                file,
                password,
                salt,
                nonce,
                identities,
                trusted,
            })
        } else {
            let file = OpenOptions::new().write(true).create(true).open(path)?;
            let mut identity_file = Self {
                file,
                password: password.into(),
                salt: pwhash::gen_salt(),
                nonce: secretbox::gen_nonce(),
                identities: Vec::new(),
                trusted: Arc::new(Mutex::new(Vec::new())),
            };
            identity_file.write().await?;
            Ok(identity_file)
        }
    }
}

impl IdentityFile<Cursor<Vec<u8>>> {
    pub fn mock(data: Option<Vec<u8>>) -> Self {
        let password = "super_secret_password";
        if data.is_none() {
            Self {
                file: Cursor::new(vec![]),
                password: password.into(),
                salt: pwhash::gen_salt(),
                nonce: secretbox::gen_nonce(),
                identities: Vec::new(),
                trusted: Arc::new(Mutex::new(Vec::new())),
            }
        } else {
            let mut file = Cursor::new(data.unwrap());
            let salt = IdentityFile::<Cursor<Vec<u8>>>::read_salt(&mut file).expect("read_salt");
            let nonce = IdentityFile::<Cursor<Vec<u8>>>::read_nonce(&mut file).expect("read_nonce");
            let key =
                IdentityFile::<Cursor<Vec<u8>>>::derive_key(&password, salt).expect("derive_key");
            let data =
                IdentityFile::<Cursor<Vec<u8>>>::decrypt_identity_data(&mut file, nonce, key)
                    .expect("decrypt_identity_data");
            let identities: Vec<Identity> =
                decode::from_read_ref(&data).expect("decode identities");
            let trusted: Arc<Mutex<Vec<PublicIdentity>>> = Arc::new(Mutex::new(
                decode::from_read_ref(&data).expect("decode trusted"),
            ));
            Self {
                file: Cursor::new(vec![]),
                password: password.into(),
                salt,
                nonce,
                identities,
                trusted,
            }
        }
    }

    pub fn data(&mut self) -> Vec<u8> {
        self.file.clone().into_inner()
    }
}

impl<F: Read + Write + Seek> IdentityFile<F> {
    fn read_salt(file: &mut F) -> Result<Salt> {
        let mut salt = Salt([0; SALTBYTES]);
        {
            let Salt(ref mut salt_bytes) = salt;
            file.read_exact(salt_bytes)?;
        }

        Ok(salt)
    }

    fn read_nonce(file: &mut F) -> Result<secretbox::Nonce> {
        let mut nonce = secretbox::Nonce([0; box_::NONCEBYTES]);
        {
            let secretbox::Nonce(ref mut nonce_bytes) = nonce;
            file.read_exact(nonce_bytes)?;
        }

        Ok(nonce)
    }

    fn derive_key(password: &str, salt: Salt) -> Result<secretbox::Key> {
        let mut key = secretbox::Key([0; secretbox::KEYBYTES]);
        {
            let secretbox::Key(ref mut key_bytes) = key;
            pwhash::derive_key_interactive(key_bytes, password.as_bytes(), &salt).unwrap();
        }

        Ok(key)
    }

    fn decrypt_identity_data(
        file: &mut F,
        nonce: secretbox::Nonce,
        key: secretbox::Key,
    ) -> Result<Vec<u8>> {
        let mut encrypted_data = Vec::<u8>::new();
        file.read_to_end(&mut encrypted_data)?;
        let result = secretbox::open(&encrypted_data, &nonce, &key);
        if let Err(_) = result {
            return Err(Error::PasswordError(
                "Error decrypting identity file with password".into(),
            ));
        }
        Ok(result.unwrap())
    }

    pub async fn add_identity(&mut self, name: &str) -> Result<()> {
        self.identities.push(Identity::new(name));
        self.write().await?;
        Ok(())
    }

    pub fn export_public_identity(&self, name: &str) -> Result<Option<String>> {
        self.identities
            .iter()
            .find(|id| id.name == name)
            .and_then(|id| Some(id.public_identity().serialize()))
            .and_then(|res| Some(res.map(|v| base64::encode(&v))))
            .map_or(Ok(None), |v| v.map(Some))
    }

    pub fn list_identities(&self) -> Vec<(String, String)> {
        self.identities
            .iter()
            .map(|id| (id.name.clone(), id.fingerprint.clone().into()))
            .collect()
    }

    pub async fn add_trusted(&mut self, encoded: &str) -> Result<()> {
        let raw = base64::decode(encoded)?;
        let trusted: PublicIdentity = decode::from_read(raw.as_slice())?;
        self.trusted.lock().await.push(trusted);
        self.write().await?;

        Ok(())
    }

    pub async fn list_trusted(&self) -> Vec<(String, String)> {
        self.trusted
            .lock()
            .await
            .iter()
            .map(|id| (id.name.clone(), id.fingerprint.clone().into()))
            .collect()
    }

    async fn write(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write(self.salt.as_ref())?;
        self.file.write(self.nonce.as_ref())?;
        let key = IdentityFile::<F>::derive_key(&self.password, self.salt)?;
        let mut data = encode::to_vec(&self.identities)?;
        data.append(&mut encode::to_vec(&self.trusted.lock().await.deref())?);
        let encrypted = secretbox::seal(&data, &self.nonce, &key);
        self.file.write(&encrypted)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sodiumoxide::crypto::pwhash::Salt;
    use sodiumoxide::crypto::secretbox::Nonce;
    use std::path::Path;

    #[tokio::test]
    async fn test_open_identity_file() {
        let path = Path::new("/tmp/identity_file");
        if path.exists() {
            std::fs::remove_file(path).expect("Remove file");
        }
        let password: String = "super_secret_password".into();
        let salt: Salt;
        let nonce: Nonce;
        {
            let file = IdentityFile::open(path, password.clone())
                .await
                .expect("Open identity file");
            salt = file.salt;
            nonce = file.nonce;
            assert!(file.identities.is_empty());
        }
        {
            let file = IdentityFile::open(path, password.clone())
                .await
                .expect("Open identity file");
            assert_eq!(salt, file.salt);
            assert_eq!(nonce, file.nonce);
            assert!(file.identities.is_empty());
        }
        {
            let mut file = IdentityFile::open(path, password.clone())
                .await
                .expect("Open identity file");
            file.add_identity("foo").await.expect("Add identity");
        }
        {
            let file = IdentityFile::open(path, password.clone())
                .await
                .expect("Open identity file");
            assert!(file.identities.len() == 1);
            assert_eq!("foo", file.identities[0].name);
        }
        std::fs::remove_file(path).expect("Remove file");
    }

    #[tokio::test]
    async fn test_list_identities() {
        let data;
        {
            let mut file = IdentityFile::mock(None);
            assert!(file.identities.is_empty());
            file.add_identity("foo").await.expect("Add identity");
            data = file.data();
        }
        {
            let file = IdentityFile::mock(Some(data));
            let identities = file.list_identities();
            assert_eq!(1, identities.len());
            assert_eq!("foo", identities[0].0);
        }
    }

    #[tokio::test]
    async fn test_export_public_identity() {
        {
            let mut file = IdentityFile::mock(None);
            file.add_identity("foo").await.expect("Add identity");
            let public_identity = file
                .export_public_identity("foo")
                .expect("Export public identity");
            assert!(public_identity.is_some());
            assert!(public_identity.unwrap().len() > 0);
            assert!(file
                .export_public_identity("bar")
                .expect("Export public identity")
                .is_none());
        }
    }

    #[tokio::test]
    async fn test_add_trusted() {
        {
            let mut file = IdentityFile::mock(None);
            file.add_identity("foo").await.expect("Add identity");
            let public_identity = file
                .export_public_identity("foo")
                .expect("Export public identity")
                .expect("Public identity found");
            assert_eq!(0, file.list_trusted().await.len());
            file.add_trusted(&public_identity)
                .await
                .expect("Add trusted");
            let trusted = file.list_trusted().await;
            assert_eq!(1, trusted.len());
            assert_eq!("foo", trusted[0].0);
        }
    }
}
