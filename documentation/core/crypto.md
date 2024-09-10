# Cryptography (crypto)

The Cryptography component in Yagna provides essential cryptographic functions for secure communication and data handling across the platform. It ensures the confidentiality, integrity, and authenticity of data exchanged between nodes and services.

## Key Features

1. **Key Management**: Generates, stores, and manages cryptographic keys securely.
2. **Encryption/Decryption**: Provides symmetric and asymmetric encryption capabilities.
3. **Digital Signatures**: Implements signing and verification of digital signatures.
4. **Hashing**: Offers cryptographic hash functions for data integrity checks.
5. **Random Number Generation**: Provides secure random number generation for various cryptographic operations.

## Cryptographic Operations

### Key Generation

Supports the generation of various types of cryptographic keys:

1. **Symmetric Keys**: For use in symmetric encryption algorithms.
2. **Asymmetric Key Pairs**: For use in public-key cryptography, digital signatures, and key exchange.

### Encryption and Decryption

Implements both symmetric and asymmetric encryption algorithms:

1. **Symmetric Encryption**: AES (Advanced Encryption Standard) in various modes (e.g., GCM, CBC).
2. **Asymmetric Encryption**: RSA and Elliptic Curve Cryptography (ECC).

### Digital Signatures

Provides functionality for creating and verifying digital signatures:

1. **Signing**: Creates signatures using private keys.
2. **Verification**: Verifies signatures using public keys.

### Hashing

Implements cryptographic hash functions:

1. **SHA-2 Family**: SHA-256, SHA-384, SHA-512.
2. **Blake2**: Blake2b, Blake2s.

## Architecture

\```plantuml
@startuml
!define RECTANGLE class

RECTANGLE "Yagna Components" as YC
RECTANGLE "Cryptography" as CRYPTO {
  RECTANGLE "Key Manager" as KM
  RECTANGLE "Encryption Engine" as EE
  RECTANGLE "Signature Service" as SS
  RECTANGLE "Hash Service" as HS
  RECTANGLE "RNG" as RNG
}
RECTANGLE "Secure Storage" as SS

YC --> CRYPTO : Uses
KM --> CRYPTO : Manages keys
EE --> CRYPTO : Encrypts/Decrypts
SS --> CRYPTO : Signs/Verifies
HS --> CRYPTO : Computes hashes
RNG --> CRYPTO : Generates random numbers
CRYPTO --> SS : Stores sensitive data

@enduml
\```

## Integration with Other Components

The Cryptography component interacts with several other Yagna components:

1. **Network (net)**: Provides encryption for network communications.
2. **Identity Management**: Assists in generating and managing cryptographic identities.
3. **Payment**: Ensures secure handling of payment-related data.
4. **GSB (Service Bus)**: Provides encryption for inter-service communications.

## Code Example: Encrypting and Signing Data

Here's a simplified example of how the Cryptography component might be used to encrypt and sign data:

\```rust
use ya_crypto::{Crypto, EncryptionAlgorithm, SignatureAlgorithm};

async fn encrypt_and_sign_data(
    crypto: &dyn Crypto,
    data: &[u8],
    recipient_public_key: &[u8],
    sender_private_key: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Encrypt the data
    let encrypted_data = crypto.encrypt(
        data,
        recipient_public_key,
        EncryptionAlgorithm::Aes256Gcm,
    ).await?;

    // Sign the encrypted data
    let signature = crypto.sign(
        &encrypted_data,
        sender_private_key,
        SignatureAlgorithm::Ed25519,
    ).await?;

    // Combine encrypted data and signature
    let mut result = encrypted_data;
    result.extend_from_slice(&signature);

    Ok(result)
}

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crypto = // Initialize Crypto implementation
    let data = b"Sensitive information";
    let recipient_public_key = // Recipient's public key
    let sender_private_key = // Sender's private key

    let encrypted_and_signed = encrypt_and_sign_data(
        &crypto,
        data,
        recipient_public_key,
        sender_private_key,
    ).await?;

    println!("Encrypted and signed data: {:?}", encrypted_and_signed);
    Ok(())
}
\```

This example demonstrates:
1. Encrypting data using a recipient's public key.
2. Signing the encrypted data using the sender's private key.
3. Combining the encrypted data and signature for secure transmission.

The Cryptography component provides a robust foundation for securing all aspects of the Yagna platform, ensuring that data remains protected throughout its lifecycle in the decentralized compute ecosystem.