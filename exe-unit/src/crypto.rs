use crate::error::Error;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use ya_client_model::activity::encrypted::EncryptionCtx;

#[derive(Clone)]
pub struct Crypto {
    sec_key: SecretKey,
    pub_key: PublicKey,
    requestor_pub_key: PublicKey,
}

impl Crypto {
    pub fn try_new(requestor_pub_key: String) -> Result<Self, Error> {
        let req_key_vec = hex::decode(requestor_pub_key)?;
        let req_key = PublicKey::from_slice(req_key_vec.as_slice())?;

        let ec = Secp256k1::new();
        let (sec_key, pub_key) = ec.generate_keypair(&mut rand::thread_rng());

        Ok(Crypto {
            sec_key,
            pub_key,
            requestor_pub_key: req_key,
        })
    }

    pub fn try_with_keys(sec_key: String, requestor_pub_key: String) -> Result<Self, Error> {
        let req_key_vec = hex::decode(requestor_pub_key)?;
        let req_key = PublicKey::from_slice(req_key_vec.as_slice())?;

        let ec = Secp256k1::new();
        let sec_vec = hex::decode(sec_key)?;
        let sec_key = SecretKey::from_slice(sec_vec.as_slice())?;
        let pub_key = PublicKey::from_secret_key(&ec, &sec_key);

        Ok(Crypto {
            sec_key,
            pub_key,
            requestor_pub_key: req_key,
        })
    }

    pub fn try_with_keys_raw(
        sec_key: SecretKey,
        requestor_pub_key: PublicKey,
    ) -> Result<Self, Error> {
        let ec = Secp256k1::new();
        let pub_key = PublicKey::from_secret_key(&ec, &sec_key);

        Ok(Crypto {
            sec_key,
            pub_key,
            requestor_pub_key,
        })
    }

    pub fn ctx(&mut self) -> EncryptionCtx {
        EncryptionCtx::new(&self.requestor_pub_key, &self.sec_key)
    }

    pub fn public_key(&self) -> String {
        let key = self.pub_key.serialize().to_vec();
        hex::encode(&key)
    }
}
