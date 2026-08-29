//! 密钥 vault 端到端加密（v2.0 规格 §7）。
//! key = argon2id(passphrase, salt)；XChaCha20-Poly1305；AAD = 账号 id；nonce 随机。
//! passphrase 用户自管，丢失 = 云端 vault 不可恢复（UI 明示）。

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

pub const NONCE_LEN: usize = 24;
pub const SALT_LEN: usize = 32;
/// argon2id 参数（交互式场景保守取值；passphrase 强度由 UI 引导）。
const ARGON2_M_COST: u32 = 19_456; // 19 MiB
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

/// vault 明文载荷：勾选 secret 集合（值来自本地 secrets 存储，不写进 yaml 明文）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vault {
    /// 账号 id（AAD 一致性校验的一部分，防跨账号重放）。
    pub account_id: String,
    /// secret 名 → 值。
    pub secrets: Vec<(String, String)>,
}

/// 加密后的实体 data（信封 `EntityData::Encrypted`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub blob: String,
    pub salt: String,
}

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    use argon2::Algorithm::Argon2id;
    use argon2::Params;
    let params =
        Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32)).expect("argon2 params");
    let mut out = [0u8; 32];
    argon2::Argon2::new(Argon2id, argon2::Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .expect("argon2 derive");
    out
}

/// 加密 vault → blob（base64: nonce || ciphertext）。
pub fn encrypt(vault: &Vault, passphrase: &str, account_id: &str) -> Result<EncryptedBlob> {
    let mut salt = [0u8; SALT_LEN];
    fill_random(&mut salt);
    let key = derive_key(passphrase, &salt);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    fill_random(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let plain = serde_json::to_vec(vault)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("vault 序列化失败: {e}")))?;
    let ct = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plain,
                aad: account_id.as_bytes(),
            },
        )
        .map_err(|_| Error::new(ErrorCode::Protocol, "vault 加密失败"))?;
    let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ct);
    Ok(EncryptedBlob {
        blob: b64(&blob),
        salt: hex(&salt),
    })
}

/// 解密 blob；错误 passphrase / 篡改 / AAD 不符 → `CLOUD_AUTH_FAILED` 口径的人话错误。
pub fn decrypt(blob: &EncryptedBlob, passphrase: &str, account_id: &str) -> Result<Vault> {
    let raw = unb64(&blob.blob);
    if raw.len() <= NONCE_LEN {
        return Err(Error::new(ErrorCode::CloudAuthFailed, "vault 数据损坏"));
    }
    let salt = unhex(&blob.salt);
    if salt.len() != SALT_LEN {
        return Err(Error::new(ErrorCode::CloudAuthFailed, "vault salt 损坏"));
    }
    let key = derive_key(passphrase, &salt);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::from_slice(&raw[..NONCE_LEN]);
    let plain = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &raw[NONCE_LEN..],
                aad: account_id.as_bytes(),
            },
        )
        .map_err(|_| {
            Error::new(
                ErrorCode::CloudAuthFailed,
                "解密失败：passphrase 错误或数据被篡改（passphrase 丢失不可恢复）",
            )
        })?;
    serde_json::from_slice(&plain)
        .map_err(|e| Error::new(ErrorCode::CloudAuthFailed, format!("vault 内容损坏: {e}")))
}

fn fill_random(buf: &mut [u8]) {
    // OS 熵（getrandom 为 RustCrypto 全系既有依赖，零新增面）
    getrandom::getrandom(buf).expect("OS RNG");
}

fn b64(data: &[u8]) -> String {
    // 手写 base64（std，零新依赖；仅内部容器使用，非通用编码库）
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn unb64(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c).unwrap_or(0) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_roundtrip_and_wrong_passphrase() {
        let vault = Vault {
            account_id: "acc-1".into(),
            secrets: vec![("DB_PASSWORD".into(), "hunter2".into())],
        };
        let blob = encrypt(&vault, "correct horse", "acc-1").unwrap();
        // blob 不可读：明文不出现在任何输出
        assert!(!blob.blob.contains("hunter2"));
        let back = decrypt(&blob, "correct horse", "acc-1").unwrap();
        assert_eq!(back, vault);
        // 错误 passphrase
        assert!(decrypt(&blob, "wrong", "acc-1").is_err());
        // AAD 不符（跨账号重放）
        assert!(decrypt(&blob, "correct horse", "acc-2").is_err());
    }

    #[test]
    fn tamper_detected() {
        let vault = Vault {
            account_id: "a".into(),
            secrets: vec![],
        };
        let mut blob = encrypt(&vault, "p", "a").unwrap();
        // 篡改密文
        blob.blob = format!("{}XX", &blob.blob[..blob.blob.len() - 2]);
        assert!(decrypt(&blob, "p", "a").is_err());
    }
}
