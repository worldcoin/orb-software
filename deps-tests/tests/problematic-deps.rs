//! This file tests and documents the platform specific requirements of various
//! dependencies.
//!
//! Not all dependencies are created equal - some can only ever target certain
//! platforms. For the best developer experience, we want to allow *all* dependencies
//! to cross compile to aarch64-unknown-linux-gnu, and as many as possible to run
//! natively on both aarch64-unknown-linux-gnu and aarch64-apple-darwin.
//!
//! If at all possible, when writing new code, please prefer using cross-platform
//! libraries rather than linux-specific ones. For example, use `rodio` instead of
//! `alsa`.
//!
//! If you need help getting your dependency working and can't figure it out, please
//! reach out!

use rodio::{cpal::traits::HostTrait, DeviceTrait};

#[test]
#[cfg(target_os = "linux")]
fn seek_camera_test() {
    println!("Running seek-camera smoke test");
    let _mngr =
        seek_camera::manager::Manager::new().expect("Failed to use seek-camera");
}

#[test]
#[cfg(target_os = "linux")]
fn alsa_test() {
    println!("Running alsa smoke test");
    alsa::card::Iter::new()
        .for_each(|c| println!("  alsa card: {}", c.unwrap().get_name().unwrap()));
}

#[test]
fn rodio_test() {
    println!("Running rodio smoke test");
    rodio::cpal::default_host()
        .devices()
        .expect("Failed to get rodio devices")
        .for_each(|d| {
            println!(
                "  rodio device: {}",
                d.name().expect("failed to get rodio device name")
            )
        });
}

#[test]
fn libc_test() {
    println!("Running libc smoke test");
    let errno = unsafe { libc::printf(c"".as_ptr()) };
    assert!(errno >= 0);
}

#[test]
fn ring_test() {
    use ring::{
        rand,
        signature::{self, KeyPair},
    };

    println!("Running ring smoke test");
    // Generate a key pair in PKCS#8 (v2) format.
    let rng = rand::SystemRandom::new();
    let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let key_pair = signature::Ed25519KeyPair::from_pkcs8(pkcs8_bytes.as_ref()).unwrap();

    // Sign the message "hello, world".
    const MESSAGE: &[u8] = b"hello, world";
    let sig = key_pair.sign(MESSAGE);

    // Normally an application would extract the bytes of the signature and
    // send them in a protocol message to the peer(s). Here we just get the
    // public key key directly from the key pair.
    let peer_public_key_bytes = key_pair.public_key().as_ref();

    // Verify the signature of the message using the public key. Normally the
    // verifier of the message would parse the inputs to this code out of the
    // protocol message(s) sent by the signer.
    let peer_public_key =
        signature::UnparsedPublicKey::new(&signature::ED25519, peer_public_key_bytes);
    peer_public_key.verify(MESSAGE, sig.as_ref()).unwrap();
}

#[test]
fn alkali_test() {
    use alkali::asymmetric::seal;

    println!("Running alkali/libsodium smoke test");
    const MESSAGE: &str = "Encrypt this message!";
    let receiver_keypair = seal::Keypair::generate().unwrap();

    // Sender side:
    // Encrypting a message with this construction adds `OVERHEAD_LENGTH` bytes of overhead (the
    // ephemeral public key + MAC).
    let mut ciphertext = vec![0u8; MESSAGE.len() + seal::OVERHEAD_LENGTH];
    // In this construction, the sender does not generate a keypair, they just use `encrypt` to
    // encrypt the message. Once it is sent, they can't decrypt it, as the ephemeral private key is
    // erased from memory.
    seal::encrypt(
        MESSAGE.as_bytes(),
        &receiver_keypair.public_key,
        &mut ciphertext,
    )
    .unwrap();

    // Receiver side:
    let mut plaintext = vec![0u8; ciphertext.len() - seal::OVERHEAD_LENGTH];
    // The receiver does not to receive any other information from the sender besides the ciphertext
    // in order to decrypt it.
    seal::decrypt(&ciphertext, &receiver_keypair, &mut plaintext).unwrap();
    assert_eq!(&plaintext, MESSAGE.as_bytes());
}

#[test]
fn openssl_test() {
    use openssl::rsa::{Padding, Rsa};

    let rsa = Rsa::generate(2048).unwrap();
    let plaintext = b"foobar";
    let mut buf = vec![0; rsa.size() as usize];
    let encrypted_len = rsa
        .public_encrypt(plaintext, &mut buf, Padding::PKCS1)
        .unwrap();
    let ciphertext = buf[0..encrypted_len].to_vec();

    let mut buf = vec![0; rsa.size() as usize];
    let decrypted_len = rsa
        .private_decrypt(&ciphertext, &mut buf, Padding::PKCS1)
        .unwrap();

    assert_eq!(plaintext, &buf[0..decrypted_len]);
}
