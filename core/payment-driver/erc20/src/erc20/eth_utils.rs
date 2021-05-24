// PRIVATE RawTransaction.hash()

use ethereum_tx_sign::RawTransaction;
use ethereum_types::U256;
use rlp::RlpStream;
use tiny_keccak::{Hasher, Keccak};

pub fn get_tx_hash(tx: &RawTransaction, chain_id: u64) -> Vec<u8> {
    let mut hash = RlpStream::new();
    hash.begin_unbounded_list();
    tx_encode(tx, &mut hash);
    hash.append(&chain_id.clone());
    hash.append(&U256::zero());
    hash.append(&U256::zero());
    hash.finalize_unbounded_list();
    keccak256_hash(&hash.out())
}

fn keccak256_hash(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    let mut resp: [u8; 32] = Default::default();
    hasher.finalize(&mut resp);
    resp.iter().cloned().collect()
}

fn tx_encode(tx: &RawTransaction, s: &mut RlpStream) {
    s.append(&tx.nonce);
    s.append(&tx.gas_price);
    s.append(&tx.gas);
    if let Some(ref t) = tx.to {
        s.append(t);
    } else {
        s.append(&vec![]);
    }
    s.append(&tx.value);
    s.append(&tx.data);
}

// MISSING RawTransaction.encode_signed_tx()

pub fn encode_signed_tx(raw_tx: &RawTransaction, signature: Vec<u8>, chain_id: u64) -> Vec<u8> {
    let (sig_v, sig_r, sig_s) = prepare_signature(signature, chain_id);

    let mut tx = RlpStream::new();

    tx.begin_unbounded_list();

    tx_encode(&raw_tx, &mut tx);
    tx.append(&sig_v);
    tx.append(&sig_r);
    tx.append(&sig_s);

    tx.finalize_unbounded_list();

    tx.out().to_vec()
}

fn prepare_signature(signature: Vec<u8>, chain_id: u64) -> (u64, Vec<u8>, Vec<u8>) {
    // TODO ugly solution
    assert_eq!(signature.len(), 65);

    let sig_v = signature[0];
    let sig_v = sig_v as u64 + chain_id * 2 + 35;

    let mut sig_r = signature.to_owned().split_off(1);
    let mut sig_s = sig_r.split_off(32);

    prepare_signature_part(&mut sig_r);
    prepare_signature_part(&mut sig_s);

    (sig_v, sig_r, sig_s)
}

fn prepare_signature_part(part: &mut Vec<u8>) {
    assert_eq!(part.len(), 32);
    while part[0] == 0 {
        part.remove(0);
    }
}

// MISSING contract.encode()

use ethabi::Error;
use web3::contract::tokens::Tokenize;
use web3::contract::Contract;
use web3::Transport;

pub fn contract_encode<P, T>(
    contract: &Contract<T>,
    func: &str,
    params: P,
) -> Result<Vec<u8>, Error>
where
    P: Tokenize,
    T: Transport,
{
    contract
        .abi()
        .function(func)
        .and_then(|function| function.encode_input(&params.into_tokens()))
}
