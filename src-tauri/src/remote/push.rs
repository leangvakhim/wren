//! Web Push to paired phones, sent straight from this computer (no Wren
//! server). The message is encrypted for the phone's browser (RFC 8291,
//! aes128gcm), so Apple / Google / Mozilla's push services that deliver it
//! can't read it. Wren identifies itself with its own VAPID key (RFC 8292).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use p256::SecretKey;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde::{Deserialize, Serialize};

use super::crypto::{b64, ecdh, hkdf, public_key, public_raw, random, unb64};

/// What the phone's browser gave Wren when it subscribed (PushSubscription.toJSON()).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Subscription {
    pub endpoint: String,
    /// The browser's public key (base64url, 65 bytes).
    pub p256dh: String,
    /// The browser's auth secret (base64url, 16 bytes).
    pub auth: String,
}

const RECORD_SIZE: u32 = 4096;

/// RFC 8291 encryption with a given sender key and salt (random in real use).
fn encrypt_with(sub_key: &[u8], auth: &[u8], plain: &[u8], sender: &SecretKey, salt: &[u8; 16]) -> Result<Vec<u8>, String> {
    let ua = public_key(sub_key)?;
    if auth.len() != 16 {
        return Err("bad push auth secret".into());
    }
    if plain.len() + 17 > RECORD_SIZE as usize {
        return Err("push message too long".into());
    }
    let as_pub = public_raw(sender);
    let shared = ecdh(sender, &ua);
    // IKM = HKDF(auth, ecdh, "WebPush: info\0" ‖ ua_public ‖ as_public)
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(sub_key);
    info.extend_from_slice(&as_pub);
    let mut ikm = [0u8; 32];
    hkdf(auth, &shared, &info, &mut ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", &mut cek);
    hkdf(salt, &ikm, b"Content-Encoding: nonce\0", &mut nonce);
    let mut record = plain.to_vec();
    record.push(2); // last (and only) record
    let ct = Aes128Gcm::new_from_slice(&cek)
        .expect("16-byte key")
        .encrypt(&Nonce::from(nonce), record.as_slice())
        .map_err(|_| "push encryption failed".to_string())?;
    let mut out = Vec::with_capacity(86 + ct.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(as_pub.len() as u8);
    out.extend_from_slice(&as_pub);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn encrypt(sub: &Subscription, plain: &[u8]) -> Result<Vec<u8>, String> {
    let sender = super::crypto::secret_key(&super::crypto::new_secret())?;
    encrypt_with(&unb64(&sub.p256dh)?, &unb64(&sub.auth)?, plain, &sender, &random::<16>())
}

/// "https://web.push.apple.com" for an endpoint (the JWT's audience).
fn origin(endpoint: &str) -> Result<String, String> {
    let rest = endpoint.strip_prefix("https://").ok_or("push endpoints must be https")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err("bad push endpoint".into());
    }
    Ok(format!("https://{host}"))
}

/// The VAPID header value for one push service.
pub fn vapid_header(vapid: &SecretKey, endpoint: &str, subject: &str, now: u64) -> Result<String, String> {
    let header = b64(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = serde_json::json!({ "aud": origin(endpoint)?, "exp": now + 12 * 3600, "sub": subject });
    let claims = b64(claims.to_string().as_bytes());
    let signing = format!("{header}.{claims}");
    let sig: Signature = SigningKey::from(vapid).sign(signing.as_bytes());
    Ok(format!("vapid t={signing}.{}, k={}", b64(&sig.to_bytes()), b64(&public_raw(vapid))))
}

/// Only push services we know (a subscription can't make Wren post anywhere else).
pub fn known_service(endpoint: &str) -> bool {
    let Ok(o) = origin(endpoint) else { return false };
    let host = &o["https://".len()..];
    let ok = |suffix: &str| host == suffix || host.ends_with(&format!(".{suffix}"));
    ok("push.apple.com") || ok("googleapis.com") || ok("mozilla.com") || ok("notify.windows.com") || ok("push.services.mozilla.com")
}

pub enum Outcome {
    Sent,
    /// The phone unsubscribed (or the browser data was cleared): forget it.
    Gone,
    Failed(String),
}

pub async fn send(client: &reqwest::Client, vapid: &SecretKey, subject: &str, sub: &Subscription, payload: &serde_json::Value) -> Outcome {
    if !known_service(&sub.endpoint) {
        return Outcome::Gone;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let body = match encrypt(sub, payload.to_string().as_bytes()) {
        Ok(b) => b,
        Err(e) => return Outcome::Failed(e),
    };
    let auth = match vapid_header(vapid, &sub.endpoint, subject, now) {
        Ok(a) => a,
        Err(e) => return Outcome::Failed(e),
    };
    let res = client
        .post(&sub.endpoint)
        .header("Authorization", auth)
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header("TTL", "86400")
        .header("Urgency", "high")
        .body(body)
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => Outcome::Sent,
        Ok(r) if r.status().as_u16() == 404 || r.status().as_u16() == 410 => Outcome::Gone,
        Ok(r) => Outcome::Failed(format!("push service answered {}", r.status())),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::crypto::{hmac, secret_key};

    /// RFC 8291 Appendix A.
    #[test]
    fn rfc8291_example() {
        let d = |s: &str| unb64(&s.replace(' ', "")).unwrap();
        let sender = secret_key(&d("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        assert_eq!(public_raw(&sender), d("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"));
        let ua = d("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let plain = d("V2hlbiBJIGdyb3cgdXAsIEkgd2FudCB0byBiZSBhIHdhdGVybWVsb24");
        let out = encrypt_with(&ua, &auth, &plain, &sender, &salt).unwrap();
        let header = d("DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
        let ct = d("8pfeW0KbunFT06SuDKoJH9Ql87S1QUrdirN6GcG7sFz1y1sqLgVi1VhjVkHsUoEsbI_0LpXMuGvnzQ");
        assert_eq!(&out[..86], header.as_slice());
        assert_eq!(&out[86..], ct.as_slice());
        // Also the PRK_key step on its own.
        let ua_priv = secret_key(&d("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94")).unwrap();
        let shared = ecdh(&ua_priv, &sender.public_key());
        assert_eq!(shared.to_vec(), d("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs"));
        assert_eq!(hmac(&auth, &[&shared]).to_vec(), d("Snr3JMxaHVDXHWJn5wdC52WjpCtd2EIEGBykDcZW32k"));
    }

    #[test]
    fn vapid_jwt_verifies() {
        use p256::ecdsa::VerifyingKey;
        use p256::ecdsa::signature::Verifier;
        let key = secret_key(&crate::remote::crypto::new_secret()).unwrap();
        let h = vapid_header(&key, "https://web.push.apple.com/QGuQyavXutnMH", "https://relay.example", 1000).unwrap();
        let t = h.strip_prefix("vapid t=").unwrap().split(", k=").next().unwrap();
        let (signed, sig) = t.rsplit_once('.').unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&unb64(signed.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://web.push.apple.com");
        assert_eq!(claims["exp"], 1000 + 12 * 3600);
        let sig = Signature::from_slice(&unb64(sig).unwrap()).unwrap();
        VerifyingKey::from(key.public_key()).verify(signed.as_bytes(), &sig).unwrap();
    }

    #[test]
    fn only_real_push_services() {
        assert!(known_service("https://web.push.apple.com/abc"));
        assert!(known_service("https://fcm.googleapis.com/fcm/send/abc"));
        assert!(known_service("https://updates.push.services.mozilla.com/wpush/v2/abc"));
        assert!(!known_service("https://evil.example/googleapis.com"));
        assert!(!known_service("https://googleapis.com.evil.example/x"));
        assert!(!known_service("http://fcm.googleapis.com/x"));
        assert!(!known_service("https://user@fcm.googleapis.com/x"));
    }
}
