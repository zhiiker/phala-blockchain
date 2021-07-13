use crate::std::string::String;
use crate::std::vec::Vec;
use anyhow::{Context, Result};
use core::fmt;
use serde::{Deserialize, Serialize};

use sp_core::crypto::Pair;
use parity_scale_codec::{Decode, Encode};

pub mod aead;
pub mod ecdh;

#[derive(Serialize, Deserialize, Debug, Clone, Encode, Decode)]
pub struct AeadCipher {
    pub iv_b64: String,
    pub cipher_b64: String,
    pub pubkey_b64: String,
}

pub struct DecryptOutput {
    pub msg: Vec<u8>,
    pub secret: Vec<u8>,
}

// Decrypt by AEAD-AES-GCM with secret key agreeded by ECDH.
pub fn decrypt(
    cipher: &AeadCipher,
    privkey: &ring::agreement::EphemeralPrivateKey,
) -> Result<DecryptOutput> {
    let pubkey = base64::decode(&cipher.pubkey_b64)
        .map_err(|_| anyhow::Error::msg(Error::BadInput("pubkey_b64")))?;
    let mut data = base64::decode(&cipher.cipher_b64)
        .map_err(|_| anyhow::Error::msg(Error::BadInput("cipher_b64")))?;
    let iv = base64::decode(&cipher.iv_b64)
        .map_err(|_| anyhow::Error::msg(Error::BadInput("iv_b64")))?;
    // ECDH derived secret
    let secret = ecdh::agree(privkey, &pubkey);
    log::info!("Agreed SK: {:?}", hex::encode(&secret));
    let msg = aead::decrypt(iv.as_slice(), secret.as_slice(), &mut data);
    Ok(DecryptOutput {
        msg: msg.to_vec(),
        secret,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Origin {
    pub origin: String,
    pub sig_b64: String,
    pub sig_type: SignatureType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SignatureType {
    #[serde(rename = "ed25519")]
    Ed25519,
    #[serde(rename = "sr25519")]
    Sr25519,
    #[serde(rename = "ecdsa")]
    Ecdsa,
}

impl Origin {
    pub fn verify(&self, msg: &[u8]) -> Result<bool> {
        let sig = base64::decode(&self.sig_b64)
            .map_err(|_| anyhow::Error::msg(Error::BadInput("sig_b64")))?;
        let pubkey: Vec<_> = hex::decode(&self.origin)
            .map_err(anyhow::Error::msg)
            .context("Failed to decode origin hex")?;

        let result = match self.sig_type {
            SignatureType::Ed25519 => verify::<sp_core::ed25519::Pair>(&sig, msg, &pubkey),
            SignatureType::Sr25519 => verify::<sp_core::sr25519::Pair>(&sig, msg, &pubkey),
            SignatureType::Ecdsa => verify::<sp_core::ecdsa::Pair>(&sig, msg, &pubkey),
        };
        Ok(result)
    }
}

fn verify<T>(sig: &[u8], msg: &[u8], pubkey: &[u8]) -> bool
where
    T: Pair,
{
    T::verify_weak(sig, msg, pubkey)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    BadInput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadInput(e) => write!(f, "bad input: {}", e),
        }
    }
}
