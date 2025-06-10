// Code from: https://github.com/tiagorangel1/cap/blob/main/wasm/src/rust/src/lib.rs

use sha2::{Digest, Sha256};

pub fn solve_pow(salt: &str, target: &str) -> u64 {
    let salt_bytes = salt.as_bytes();

    let target_bytes = parse_hex_target(target);
    let target_bits = target.len() * 4; // each hex char = 4 bits

    let mut nonce_buffer = [0u8; 20]; // u64::MAX has at most 20 digits

    for nonce in 0..u64::MAX {
        let nonce_len = write_u64_to_buffer(nonce, &mut nonce_buffer);
        let nonce_bytes = &nonce_buffer[..nonce_len];

        let mut hasher = Sha256::new();
        hasher.update(salt_bytes);
        hasher.update(nonce_bytes);
        let hash_result = hasher.finalize();

        if hash_matches_target(&hash_result, &target_bytes, target_bits) {
            return nonce;
        }
    }

    unreachable!("Solution should be found before exhausting u64::MAX");
}

fn parse_hex_target(target: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity((target.len() + 1) / 2);
    let chars: Vec<char> = target.chars().collect();

    for chunk in chars.chunks(2) {
        let hex_str: String = chunk.iter().collect();
        if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
            result.push(byte);
        }
    }
    result
}

fn write_u64_to_buffer(mut value: u64, buffer: &mut [u8]) -> usize {
    if value == 0 {
        buffer[0] = b'0';
        return 1;
    }

    let mut len = 0;
    let mut temp = value;

    while temp > 0 {
        len += 1;
        temp /= 10;
    }

    for i in (0..len).rev() {
        buffer[i] = (value % 10) as u8 + b'0';
        value /= 10;
    }

    len
}

fn hash_matches_target(hash: &[u8], target_bytes: &[u8], target_bits: usize) -> bool {
    let full_bytes = target_bits / 8;
    let remaining_bits = target_bits % 8;

    if hash[..full_bytes] != target_bytes[..full_bytes] {
        return false;
    }

    if remaining_bits > 0 && full_bytes < target_bytes.len() {
        let mask = 0xFF << (8 - remaining_bits);
        let hash_masked = hash[full_bytes] & mask;
        let target_masked = target_bytes[full_bytes] & mask;
        return hash_masked == target_masked;
    }

    true
}
