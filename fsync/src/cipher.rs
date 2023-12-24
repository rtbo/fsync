use std::str;

use aes::cipher::{KeyIvInit, StreamCipher};
use base64::prelude::*;
use rand::{rngs::OsRng, RngCore};

type Aes256Ctr64LE = ctr::Ctr64LE<aes::Aes256>;

const IV_LEN: usize = 16;
const KEY: &[u8; 32] = include_bytes!("cipher.binkey");

pub fn cipher_text(cleartext: &str) -> String {
    let mut iv = [0u8; IV_LEN];
    let mut gen = OsRng {};
    gen.fill_bytes(&mut iv[..]);

    let mut ciphertext = vec![0u8; cleartext.len() + IV_LEN];
    ciphertext[..IV_LEN].copy_from_slice(&iv[..]);

    let mut cipher = Aes256Ctr64LE::new(KEY.into(), &iv.into());
    cipher.apply_keystream_b2b(cleartext.as_bytes(), &mut ciphertext[IV_LEN..]).unwrap();

    BASE64_STANDARD_NO_PAD.encode(ciphertext)
}

pub fn decipher_text(ciphertext: &str) -> String {
    let ciphertext = BASE64_STANDARD_NO_PAD
        .decode(ciphertext)
        .expect("invalid ciphertext (not base64)");

    let mut iv = [0u8; IV_LEN];
    iv.copy_from_slice(&ciphertext[..IV_LEN]);

    let mut cleartext = vec![0u8; ciphertext.len() - IV_LEN];

    let mut cipher = Aes256Ctr64LE::new(KEY.into(), &iv.into());
    cipher.apply_keystream_b2b(&ciphertext[IV_LEN..], &mut cleartext[..]).unwrap();

    String::from_utf8(cleartext).expect("wrong deciphered text (not utf-8)")
}

#[test]
fn test_cipher_lengths() {
    // respectively 15, 16, 17, 31, 32, 33 bytes
    let msgs = [
        "0123456789abcde",
        "0123456789abcdef",
        "0123456789abcdefg",
        "0123456789abcdefghijklmnopqrstu",
        "0123456789abcdefghijklmnopqrstuv",
        "0123456789abcdefghijklmnopqrstuvw",
    ];

    for msg in msgs {
        let ciphertext = cipher_text(msg);
        let cleartext = decipher_text(&ciphertext);
        assert_eq!(msg, &cleartext);
    }
}

#[test]
fn test_cipher_repeat() {
    let msg = "a string message";

    let cipher1 = cipher_text(msg);
    let cipher2 = cipher_text(msg);
    assert_ne!(&cipher1, &cipher2);

    let clear1 = decipher_text(&cipher1);
    let clear2 = decipher_text(&cipher2);
    assert_eq!(&clear1, msg);
    assert_eq!(&clear2, msg);
}
