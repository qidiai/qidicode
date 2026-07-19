//! B2: 供应链完整性校验（自更新二进制）。
//!
//! 下载的二进制在原子替换（[`crate::auto_update`] 的 `swap_managed_bin_links`）
//! 前，必须用 Ed25519 detached signature 校验通过才执行替换。受信公钥编译进
//! 二进制（`trusted_pubkey.bin`），对应私钥只存在于发布方的签名 secret 中，
//! 不进仓库。
//!
//! ## 未配置密钥前保持惰性
//!
//! 与 `cf-config` 的 `signed_policy` 一致：本 fork 尚未建立签名基础设施，仓库
//! 内的 `trusted_pubkey.bin` 是全零占位符，[`verification_active`] 返回 false，
//! 调用方跳过签名校验（下载完整性仍由 smoke-test 兜底），从而不破坏尚未发布
//! `.sig` 的自更新链路，也避免"以为有签名但实际无密钥"的虚假安全感。
//!
//! 一旦把真实 32 字节 Ed25519 公钥写入 `trusted_pubkey.bin` 并重新编译，
//! [`verification_active`] 即为 true，自更新流程会在替换前强制校验
//! `<binary>.sig`，校验失败则保留旧版本、拒绝替换。

use ed25519_dalek::{Signature, SignatureError, Verifier, VerifyingKey};

/// 编译期嵌入的受信 Ed25519 公钥（原始 32 字节）。
///
/// 用 `include_bytes!` 从 crate 根的 `trusted_pubkey.bin` 加载——注意不是
/// `include!`（后者会把文件内容当 Rust 源码解析）。密钥轮换时替换该文件并
/// 重新编译。
///
/// WARNING: 当前仓库内为全零占位符，发布前必须替换为真实公钥；在替换前
/// [`verification_active`] 返回 false，签名校验保持关闭。
pub const TRUSTED_PUBKEY: &[u8; 32] = include_bytes!("../trusted_pubkey.bin");

/// 全零占位符——表示尚未配置真实签名密钥。
const PLACEHOLDER_PUBKEY: [u8; 32] = [0u8; 32];

/// 是否已配置真实受信公钥（即签名校验是否生效）。
///
/// 占位符（全零）时返回 false，调用方据此跳过签名校验。
pub fn verification_active() -> bool {
    *TRUSTED_PUBKEY != PLACEHOLDER_PUBKEY
}

/// 用编译期受信公钥校验 `blob` 与其 detached 签名 `sig_bytes`。
///
/// 返回 `Ok(())` 表示完整性确认；`Err` 表示校验失败，调用方必须拒绝替换。
pub fn verify_download(blob: &[u8], sig_bytes: &[u8]) -> Result<(), SignatureError> {
    verify_with_key(TRUSTED_PUBKEY, blob, sig_bytes)
}

/// 纯校验核心：调用方显式提供公钥，便于单元测试用临时密钥对覆盖，而不依赖
/// 仓库里的占位符文件。
fn verify_with_key(
    pubkey: &[u8; 32],
    blob: &[u8],
    sig_bytes: &[u8],
) -> Result<(), SignatureError> {
    let sig = Signature::from_slice(sig_bytes)?;
    let key = VerifyingKey::from_bytes(pubkey)?;
    key.verify(blob, &sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// 生成确定性的测试密钥对（不依赖随机源，测试可复现）。
    fn test_keypair() -> (SigningKey, [u8; 32]) {
        let mut seed = [0u8; 32];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(1);
        }
        let sk = SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key().to_bytes();
        (sk, pk)
    }

    #[test]
    fn valid_signature_passes() {
        let (sk, pk) = test_keypair();
        let msg = b"hello binary";
        let sig = sk.sign(msg);
        assert!(verify_with_key(&pk, msg, &sig.to_bytes()).is_ok());
    }

    #[test]
    fn tampered_blob_rejected() {
        let (sk, pk) = test_keypair();
        let msg = b"hello binary";
        let sig = sk.sign(msg);
        let mut tampered = msg.to_vec();
        tampered[0] ^= 0x01;
        assert!(verify_with_key(&pk, &tampered, &sig.to_bytes()).is_err());
    }

    #[test]
    fn tampered_signature_rejected() {
        let (sk, pk) = test_keypair();
        let msg = b"hello binary";
        let mut sig_bytes = sk.sign(msg).to_bytes();
        sig_bytes[0] ^= 0x01;
        assert!(verify_with_key(&pk, msg, &sig_bytes).is_err());
    }

    #[test]
    fn wrong_key_rejected() {
        let (sk, _pk) = test_keypair();
        let msg = b"hello binary";
        let sig = sk.sign(msg);
        let other_pk = SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes();
        assert!(verify_with_key(&other_pk, msg, &sig.to_bytes()).is_err());
    }

    #[test]
    fn malformed_signature_length_rejected() {
        let (_sk, pk) = test_keypair();
        // 长度不对的签名必须被拒绝而非 panic。
        assert!(verify_with_key(&pk, b"msg", &[0u8; 10]).is_err());
    }

    #[test]
    fn verification_inactive_with_placeholder_pubkey() {
        // 仓库内为全零占位符，签名校验默认关闭（见模块文档）。
        assert!(!verification_active());
        assert_eq!(*TRUSTED_PUBKEY, PLACEHOLDER_PUBKEY);
    }
}
