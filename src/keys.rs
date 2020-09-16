use serde_derive::{Deserialize, Serialize};
use sodiumoxide::crypto::{box_, kx, secretbox, secretstream};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    pub fn from_slice(bs: &[u8]) -> Option<Self> {
        if bs.len() != 32 {
            return None;
        }

        let mut n = PublicKey([0; 32]);
        n.0.copy_from_slice(bs);

        Some(n)
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecretKey(pub [u8; 32]);

impl SecretKey {
    pub fn from_slice(bs: &[u8]) -> Option<Self> {
        if bs.len() != 32 {
            return None;
        }

        let mut n = SecretKey([0; 32]);
        n.0.copy_from_slice(bs);

        Some(n)
    }
}

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SymmetricKey(pub [u8; 32]);

impl SymmetricKey {
    pub fn from_slice(bs: &[u8]) -> Option<Self> {
        if bs.len() != 32 {
            return None;
        }

        let mut n = SymmetricKey([0; 32]);
        n.0.copy_from_slice(bs);

        Some(n)
    }
}

impl AsRef<[u8]> for SymmetricKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Nonce(pub [u8; 24]);

impl Nonce {
    pub fn from_slice(bs: &[u8]) -> Option<Self> {
        if bs.len() != 24 {
            return None;
        }

        let mut n = Nonce([0; 24]);
        n.0.copy_from_slice(bs);

        Some(n)
    }
}

impl AsRef<[u8]> for Nonce {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub fn gen_nonce() -> Nonce {
    box_::gen_nonce().into()
}

impl From<box_::Nonce> for Nonce {
    fn from(nonce: box_::Nonce) -> Nonce {
        Nonce::from_slice(nonce.as_ref()).unwrap()
    }
}

impl From<Nonce> for box_::Nonce {
    fn from(nonce: Nonce) -> box_::Nonce {
        box_::Nonce::from_slice(nonce.as_ref()).unwrap()
    }
}

impl From<box_::PublicKey> for PublicKey {
    fn from(pk: box_::PublicKey) -> PublicKey {
        PublicKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<PublicKey> for box_::PublicKey {
    fn from(pk: PublicKey) -> box_::PublicKey {
        box_::PublicKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<kx::PublicKey> for PublicKey {
    fn from(pk: kx::PublicKey) -> PublicKey {
        PublicKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<PublicKey> for kx::PublicKey {
    fn from(pk: PublicKey) -> kx::PublicKey {
        kx::PublicKey::from_slice(pk.as_ref()).unwrap()
    }
}

pub fn gen_kx_keypair() -> (PublicKey, SecretKey) {
    let (pk, sk) = kx::gen_keypair();
    (pk.into(), sk.into())
}

pub fn gen_box_keypair() -> (PublicKey, SecretKey) {
    let (pk, sk) = box_::gen_keypair();
    (pk.into(), sk.into())
}

impl From<box_::SecretKey> for SecretKey {
    fn from(pk: box_::SecretKey) -> SecretKey {
        SecretKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<SecretKey> for box_::SecretKey {
    fn from(pk: SecretKey) -> box_::SecretKey {
        box_::SecretKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<kx::SecretKey> for SecretKey {
    fn from(pk: kx::SecretKey) -> SecretKey {
        SecretKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<SecretKey> for kx::SecretKey {
    fn from(pk: SecretKey) -> kx::SecretKey {
        kx::SecretKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<secretbox::Key> for SymmetricKey {
    fn from(pk: secretbox::Key) -> SymmetricKey {
        SymmetricKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<SymmetricKey> for secretbox::Key {
    fn from(pk: SymmetricKey) -> secretbox::Key {
        secretbox::Key::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<secretstream::Key> for SymmetricKey {
    fn from(pk: secretstream::Key) -> SymmetricKey {
        SymmetricKey::from_slice(pk.as_ref()).unwrap()
    }
}

impl From<SymmetricKey> for secretstream::Key {
    fn from(pk: SymmetricKey) -> secretstream::Key {
        secretstream::Key::from_slice(pk.as_ref()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sodiumoxide::crypto::{box_, kx, secretbox};

    #[test]
    fn test_key_interop() {
        let (our_pk, our_sk) = gen_kx_keypair();
        let (their_pk, their_sk) = gen_kx_keypair();
        let nonce = box_::gen_nonce();
        let plaintext = b"plaintext-test";
        let ciphertext = box_::seal(plaintext, &nonce, &their_pk.into(), &our_sk.into());
        let my_plaintext =
            box_::open(&ciphertext, &nonce, &our_pk.into(), &their_sk.into()).expect("open");
        assert_eq!(plaintext, &my_plaintext[..]);
    }
}
