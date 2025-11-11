// RFC7636: https://datatracker.ietf.org/doc/html/rfc7636

use rand::{RngCore, SeedableRng, rngs::StdRng};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

type CodeVerifier = String;
type CodeChallenge = String;

fn code_verifier() -> CodeVerifier {
    let mut rng = StdRng::from_os_rng();
    let mut payload: [u8; 32] = [0; _];
    rng.fill_bytes(&mut payload);

    BASE64_URL_SAFE_NO_PAD.encode(payload)
}

fn code_challenge(ver: &CodeVerifier) -> CodeChallenge {
    BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(ascii(ver)))
}

fn ascii(s: &CodeVerifier) -> Box<[u8]> {
    assert!(s.is_ascii());
    s.bytes().collect()
}

pub fn generate() -> (CodeVerifier, CodeChallenge) {
    let cv = code_verifier();
    let cc = code_challenge(&cv);

    (cv, cc)
}