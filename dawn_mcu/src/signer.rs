use ed25519_dalek::{Signature, Signer, SigningKey};
use rand_core::OsRng;

/// Generates a keypair representing the hardware security module of the MCU.
pub fn generate_mcu_keypair() -> SigningKey {
    let mut csprng = OsRng;
    SigningKey::generate(&mut csprng)
}

/// Helper to get the DID representation of the public key.
pub fn get_public_did(signing_key: &SigningKey) -> String {
    let verifying_key = signing_key.verifying_key();
    let pub_key_bytes = verifying_key.as_bytes();
    // Simplified DID hex string for the demonstration
    format!("did:dawn:mcu:{}", hex::encode(pub_key_bytes))
}

/// Signs a specific payload for the Agentic Payments Protocol (AP2) Verifiable Credential.
pub fn sign_payload(signing_key: &SigningKey, payload: &str) -> String {
    let signature: Signature = signing_key.sign(payload.as_bytes());
    hex::encode(&signature.to_bytes())
}

// Simple hex encoder to avoid pulling in full hex crate for demonstration
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    }
}
