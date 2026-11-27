//! End-to-end encryption between Wren and a paired phone. The relay in the
//! middle only ever sees `hello` / `welcome` (public keys and random nonces)
//! and then opaque boxes. The phone side is phone/src/protocol.ts; both must
//! stay byte-for-byte the same (a test runs the JS side against this file).
//!
//! - Keys: P-256. Wren has one long-lived key (its public half is in the QR
//!   code); each phone makes its own non-extractable key in the browser.
//! - Pairing proof: HMAC-SHA256(pair secret, "wren-pair-v1" ‖ phone key ‖ phone nonce).
//!   The secret is only in the QR code's URL fragment, which browsers never
//!   send to a server, so the relay can't swap in its own key.
//! - Per connection: ECDH → HKDF-SHA256(salt = phone nonce ‖ Wren nonce,
//!   info = "wren-remote-v1") → 64 bytes: Wren→phone key, phone→Wren key.
//! - Messages: AES-256-GCM, IV = 4 zero bytes ‖ 64-bit message counter. The
//!   counter is never sent: both sides count, so a replayed, dropped or
//!   reordered message fails to open and the connection is closed.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use p256::{PublicKey, SecretKey};
use sha2::Sha256;

pub const PAIR_LABEL: &[u8] = b"wren-pair-v1";
pub const KDF_INFO: &[u8] = b"wren-remote-v1";

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).map_err(|_| "bad base64".to_string())
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system's random number generator failed");
    b
}

/// A new P-256 private key (raw 32 bytes).
pub fn new_secret() -> [u8; 32] {
    loop {
        let b = random::<32>();
        if SecretKey::from_slice(&b).is_ok() {
            return b;
        }
    }
}

pub fn secret_key(raw: &[u8]) -> Result<SecretKey, String> {
    SecretKey::from_slice(raw).map_err(|_| "bad private key".to_string())
}

/// Uncompressed SEC1 public key (65 bytes), the format WebCrypto's "raw" export uses.
pub fn public_raw(secret: &SecretKey) -> Vec<u8> {
    secret.public_key().to_sec1_bytes().to_vec()
}

pub fn public_key(raw: &[u8]) -> Result<PublicKey, String> {
    if raw.len() != 65 || raw[0] != 4 {
        return Err("bad public key".into());
    }
    PublicKey::from_sec1_bytes(raw).map_err(|_| "bad public key".to_string())
}

/// The x coordinate of the shared point (= WebCrypto deriveBits(256)).
pub fn ecdh(secret: &SecretKey, peer: &PublicKey) -> [u8; 32] {
    let shared = secret.diffie_hellman(peer);
    let mut out = [0u8; 32];
    out.copy_from_slice(shared.raw_secret_bytes());
    out
}

#[cfg(test)]
pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

pub fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(Some(salt), ikm).expand(info, out).expect("HKDF output length is valid");
}

/// The proof a phone sends with its first `hello` after scanning the QR code
/// (made by phone/src/protocol.ts; here for tests).
#[cfg(test)]
pub fn pair_proof(secret: &[u8], phone_pub: &[u8], phone_nonce: &[u8]) -> [u8; 32] {
    hmac(secret, &[PAIR_LABEL, phone_pub, phone_nonce])
}

/// Constant-time check of a pairing proof.
pub fn pair_proof_ok(secret: &[u8], phone_pub: &[u8], phone_nonce: &[u8], proof: &[u8]) -> bool {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(PAIR_LABEL);
    mac.update(phone_pub);
    mac.update(phone_nonce);
    mac.verify_slice(proof).is_ok()
}

/// One direction of an encrypted connection.
pub struct Box1 {
    cipher: Aes256Gcm,
    counter: u64,
}

impl Box1 {
    fn new(key: &[u8]) -> Self {
        Self { cipher: Aes256Gcm::new_from_slice(key).expect("32-byte key"), counter: 0 }
    }

    fn iv(&self) -> [u8; 12] {
        let mut iv = [0u8; 12];
        iv[4..].copy_from_slice(&self.counter.to_be_bytes());
        iv
    }

    pub fn seal(&mut self, plain: &[u8]) -> String {
        let iv = self.iv();
        self.counter += 1;
        let ct = self.cipher.encrypt(&Nonce::from(iv), plain).expect("AES-GCM encrypts any length here");
        b64(&ct)
    }

    pub fn open(&mut self, sealed: &str) -> Result<Vec<u8>, String> {
        let ct = unb64(sealed)?;
        let iv = self.iv();
        let plain = self.cipher.decrypt(&Nonce::from(iv), ct.as_slice()).map_err(|_| "a message didn't decrypt".to_string())?;
        self.counter += 1;
        Ok(plain)
    }
}

/// Both directions of one phone connection, seen from one side.
pub struct Channel {
    pub send: Box1,
    pub recv: Box1,
}

/// The two keys of a connection: (Wren→phone, phone→Wren).
pub fn session_keys(shared: &[u8; 32], phone_nonce: &[u8], host_nonce: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut salt = Vec::with_capacity(phone_nonce.len() + host_nonce.len());
    salt.extend_from_slice(phone_nonce);
    salt.extend_from_slice(host_nonce);
    let mut okm = [0u8; 64];
    hkdf(&salt, shared, KDF_INFO, &mut okm);
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    a.copy_from_slice(&okm[..32]);
    b.copy_from_slice(&okm[32..]);
    (a, b)
}

impl Channel {
    /// Wren's side of a connection.
    pub fn host(host: &SecretKey, phone_pub: &PublicKey, phone_nonce: &[u8], host_nonce: &[u8]) -> Self {
        let (h2p, p2h) = session_keys(&ecdh(host, phone_pub), phone_nonce, host_nonce);
        Self { send: Box1::new(&h2p), recv: Box1::new(&p2h) }
    }

    /// The phone's side (used by tests; the real phone is phone/src/protocol.ts).
    #[cfg(test)]
    pub fn phone(phone: &SecretKey, host_pub: &PublicKey, phone_nonce: &[u8], host_nonce: &[u8]) -> Self {
        let (h2p, p2h) = session_keys(&ecdh(phone, host_pub), phone_nonce, host_nonce);
        Self { send: Box1::new(&p2h), recv: Box1::new(&h2p) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_talk_and_replays_fail() {
        let host = secret_key(&new_secret()).unwrap();
        let phone = secret_key(&new_secret()).unwrap();
        let (np, nh) = (random::<16>(), random::<16>());
        let mut h = Channel::host(&host, &phone.public_key(), &np, &nh);
        let mut p = Channel::phone(&phone, &host.public_key(), &np, &nh);
        let a = p.send.seal(b"hello wren");
        assert_eq!(h.recv.open(&a).unwrap(), b"hello wren");
        // The same box again (a replay) no longer opens.
        assert!(h.recv.open(&a).is_err());
        let b = h.send.seal(b"hi phone");
        assert_eq!(p.recv.open(&b).unwrap(), b"hi phone");
        // A different nonce = different keys.
        let mut other = Channel::phone(&phone, &host.public_key(), &random::<16>(), &nh);
        assert!(other.recv.open(&h.send.seal(b"x")).is_err());
    }

    #[test]
    fn pair_proof_checks_every_part() {
        let secret = random::<32>();
        let key = public_raw(&secret_key(&new_secret()).unwrap());
        let n = random::<16>();
        let proof = pair_proof(&secret, &key, &n);
        assert!(pair_proof_ok(&secret, &key, &n, &proof));
        assert!(!pair_proof_ok(&random::<32>(), &key, &n, &proof));
        assert!(!pair_proof_ok(&secret, &key, &random::<16>(), &proof));
        assert!(public_key(&key).is_ok());
        assert!(public_key(&key[1..]).is_err());
    }
}
