use sha3::{self, Digest};

pub trait Hasher {
    fn input(&mut self, data: &[u8]);
    fn result(&mut self) -> Vec<u8>;
}

macro_rules! impl_hasher {
    ($name:ident, $digest:path) => {
        #[derive(Default)]
        pub struct $name {
            digest: $digest,
        }

        impl Hasher for $name {
            #[inline(always)]
            fn input(&mut self, data: &[u8]) {
                self.digest.input(data);
            }

            #[inline(always)]
            fn result(&mut self) -> Vec<u8> {
                self.digest.result_reset().as_slice().to_vec()
            }
        }
    };
}

impl_hasher!(Sha3_224, sha3::Sha3_224);
impl_hasher!(Sha3_256, sha3::Sha3_256);
impl_hasher!(Sha3_384, sha3::Sha3_384);
impl_hasher!(Sha3_512, sha3::Sha3_512);
