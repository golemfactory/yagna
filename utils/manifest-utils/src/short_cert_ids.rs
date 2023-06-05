use std::collections::HashMap;

use itertools::Itertools;

pub struct Input<T> {
    pub data: T,
    pub long_cert_id: String,
}

pub struct Output<T> {
    pub data: T,
    pub short_cert_id: String,
}

pub fn shorten_cert_ids<T>(input: Vec<Input<T>>) -> Vec<Output<T>> {
    const DIGEST_PREFIX_LENGTHS: [usize; 3] = [8, 32, 128];

    // hard-code support for the use of the entire signature, regardless of its size,
    // ensure all prefixes are no longer than the signature, and remove duplicates.
    //
    // these are, by construction, sorted smallest to largest.
    let prefix_lengths = |id_len| {
        DIGEST_PREFIX_LENGTHS
            .iter()
            .map(move |&n| std::cmp::min(n, id_len))
            .chain(std::iter::once(id_len))
            .dedup()
    };

    let mut prefix_uses = HashMap::<String, u32>::new();
    for cert in &input {
        for len in prefix_lengths(cert.long_cert_id.len()) {
            let mut prefix = cert.long_cert_id.clone();
            prefix.truncate(len);

            *prefix_uses.entry(prefix).or_default() += 1;
        }
    }

    let mut ids = Vec::new();
    for cert in &input {
        for len in prefix_lengths(cert.long_cert_id.len()) {
            let mut prefix = cert.long_cert_id.clone();
            prefix.truncate(len);

            let usages = *prefix_uses
                .get(&prefix)
                .expect("Internal error, unexpected prefix");

            // the longest prefix (i.e. the entire fingerprint) will be unique, so
            // this condition is guaranteed to execute during the last iteration,
            // at the latest.
            if usages == 1 {
                ids.push(prefix);
                break;
            }
        }
    }

    let mut values = Vec::new();
    for (id_prefix, cert) in ids.into_iter().zip(input) {
        values.push(Output {
            data: cert.data,
            short_cert_id: id_prefix,
        })
    }

    values
}
