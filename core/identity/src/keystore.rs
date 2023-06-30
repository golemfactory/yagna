/// This file is just a clone of implementation from eth-keystore
/// Unfortunately eth-keystore library need file to work (which is absurd and should be fixed)
/// So the following code is a copy of the library with some modifications
/// I need to add some duplicated dependencies to get it to work, I hope it's not too bad
///
/// What's is even worse it's that this code is already in web3, but there is no access
/// for private key to retrieve, which is stupid idea of creators of the library,
/// so a lot of work and complications to get simple thing as exporting private key done.
use aes::{
    cipher::{self, InnerIvInit, KeyInit, StreamCipherCore},
    Aes128,
};
use digest::{Digest, Update};
use eth_keystore::{EthKeystore, KdfparamsType, KeystoreError};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use scrypt::scrypt;
use sha2::Sha256;
use sha3::Keccak256;
use std::convert::TryInto;

struct Aes128Ctr {
    inner: ctr::CtrCore<Aes128, ctr::flavors::Ctr128BE>,
}

impl Aes128Ctr {
    fn new(key: &[u8], iv: &[u8]) -> Result<Self, cipher::InvalidLength> {
        let cipher = aes::Aes128::new_from_slice(key).unwrap();
        let inner = ctr::CtrCore::inner_iv_slice_init(cipher, iv).unwrap();
        Ok(Self { inner })
    }

    fn apply_keystream(self, buf: &mut [u8]) {
        self.inner.apply_keystream_partial(buf.into());
    }
}

pub fn decrypt_key(keystore_str: &str, password: &str) -> Result<[u8; 32], KeystoreError> {
    let keystore: EthKeystore = serde_json::from_str(keystore_str)?;

    // Derive the key.
    let key = match keystore.crypto.kdfparams {
        KdfparamsType::Pbkdf2 {
            c,
            dklen,
            prf: _,
            salt,
        } => {
            let mut key = vec![0u8; dklen as usize];
            pbkdf2::<Hmac<Sha256>>(password.as_ref(), &salt, c, key.as_mut_slice())?;
            key
        }
        KdfparamsType::Scrypt {
            dklen,
            n,
            p,
            r,
            salt,
        } => {
            let mut key = vec![0u8; dklen as usize];
            let log_n = (n as f32).log2() as u8;
            let scrypt_params = scrypt::Params::new(log_n, r, p)?;
            scrypt(password.as_ref(), &salt, &scrypt_params, key.as_mut_slice())?;
            key
        }
    };

    // Derive the MAC from the derived key and ciphertext.
    let derived_mac = Keccak256::new()
        .chain(&key[16..32])
        .chain(&keystore.crypto.ciphertext)
        .finalize();

    if derived_mac.as_slice() != keystore.crypto.mac.as_slice() {
        return Err(KeystoreError::MacMismatch);
    }

    // Decrypt the private key bytes using AES-128-CTR
    let decryptor =
        Aes128Ctr::new(&key[..16], &keystore.crypto.cipherparams.iv[..16]).expect("invalid length");

    let mut pk = keystore.crypto.ciphertext;
    decryptor.apply_keystream(&mut pk);

    pk.try_into()
        .map_err(|_| KeystoreError::StdIo("Invalid output key length".to_string()))
}

// test key decryption

#[test]
fn test_decryption_no_password() {
    let keystore_no_password = r#"
{
  "id": "707190c0-48e7-4749-9523-57461c9780df",
  "version": 3,
  "crypto": {
    "cipher": "aes-128-ctr",
    "cipherparams": {
      "iv": "a47b5ac02ebbb8b251fb3c756f15c40d"
    },
    "ciphertext": "d10bbc8bdd563adead88e461ca9aaf88431385d5b469182762b3ea447b57b4ef",
    "kdf": "pbkdf2",
    "kdfparams": {
      "c": 10240,
      "dklen": 32,
      "prf": "hmac-sha256",
      "salt": "be2a75d09463aa43f59928d54b00cc3a00d207f1449f8271d0502ee0344a4c8c"
    },
    "mac": "34568238b389577418cef3d8a4ce026be4b6684d440da95aa458528874c2d13f"
  },
  "address": "fe51dd55acd72e84bdfa907f8fe44252e0457206"
}
    "#;
    let secret = decrypt_key(keystore_no_password, "").unwrap();
    let secret = hex::encode(secret);
    println!("Private key: {}", secret);

    assert_eq!(
        secret,
        "c057eac4ce42c1ec20e918353ccb450de9306cbdd530eb74613e207836043250"
    )
}

#[test]
fn test_decryption_password() {
    let keystore_no_password = r#"
{
  "id": "777694ee-120f-4b96-b4ff-752f46552394",
  "version": 3,
  "crypto": {
    "cipher": "aes-128-ctr",
    "cipherparams": {
      "iv": "4efee3bb098c87c56052c20036848b5a"
    },
    "ciphertext": "7ba29006b35115d9d98d08aa20804332d3147ecbb66fb5813fdc5c6ad553c605",
    "kdf": "pbkdf2",
    "kdfparams": {
      "c": 10240,
      "dklen": 32,
      "prf": "hmac-sha256",
      "salt": "b538883c64ffa4dbcea014bb42a3f498df57e54951166c078cba1ae4c63db623"
    },
    "mac": "7df44c285f0af16c58b22f434e391d51034bf0367fd61bf330d8ab02b59641be"
  },
  "address": "5afa3ec4d616b059e9f0abd77e0a336e80dfb77e"
}
    "#;
    let secret = decrypt_key(keystore_no_password, "k1h$&qT@&seZy5VS").unwrap();
    let secret = hex::encode(secret);
    println!("Private key: {}", secret);

    assert_eq!(
        secret,
        "78bf400e180af932fe6cf071eef0176fddc94cd19ad0b2937f85506b143cd36a"
    );
}
