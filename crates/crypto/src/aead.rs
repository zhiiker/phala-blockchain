use crate::CryptoError;
use alloc::vec::Vec;
use ring::aead::{LessSafeKey, UnboundKey};
// use ring::rand::SecureRandom;

// aes-256-gcm key
pub struct AeadKey(LessSafeKey);

pub const IV_BYTES: usize = 12;
pub type IV = [u8; IV_BYTES];

// pub fn generate_iv() -> IV {
//     let mut nonce_vec = [0_u8; IV_BYTES];
//     let rand = ring::rand::SystemRandom::new();
//     rand.fill(&mut nonce_vec).unwrap();
//     nonce_vec
// }

fn load_key(raw: &[u8]) -> Result<AeadKey, CryptoError> {
    let unbound_key =
        UnboundKey::new(&ring::aead::AES_256_GCM, raw).map_err(|_| CryptoError::AeadInvalidKey)?;
    Ok(AeadKey(LessSafeKey::new(unbound_key)))
}

// Encrypts the data in-place and appends a 128bit auth tag
pub fn encrypt(iv: &IV, secret: &[u8], in_out: &mut Vec<u8>) -> Result<(), CryptoError> {
    let nonce = ring::aead::Nonce::assume_unique_for_key(iv.clone());
    let key = load_key(secret)?;

    key.0
        .seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), in_out)
        .map_err(|_| CryptoError::AeadEncryptError)?;
    Ok(())
}

// Decrypts the cipher (with 128 auth tag appended) in-place and returns the message as a slice.
pub fn decrypt<'in_out>(
    iv: &[u8],
    secret: &[u8],
    in_out: &'in_out mut [u8],
) -> Result<&'in_out mut [u8], CryptoError> {
    let mut iv_arr = [0_u8; IV_BYTES];
    iv_arr.copy_from_slice(&iv[..IV_BYTES]);
    let key = load_key(secret)?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(iv_arr);

    key.0
        .open_in_place(nonce, ring::aead::Aad::empty(), in_out)
        .map_err(|_| CryptoError::AeadDecryptError)
}
