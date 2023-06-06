use std::collections::HashMap;

use itertools::Itertools;

pub fn shorten_cert_ids(long_ids: &[String]) -> Vec<String> {
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
    for long_id in long_ids {
        for len in prefix_lengths(long_id.len()) {
            let mut prefix = long_id.to_string();
            prefix.truncate(len);

            *prefix_uses.entry(prefix).or_default() += 1;
        }
    }

    let mut ids = Vec::new();
    for long_id in long_ids {
        for len in prefix_lengths(long_id.len()) {
            let mut prefix = long_id.to_string();
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

    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    use pretty_assertions::assert_eq;
    use test_case::test_case;

    #[test_case(vec!["--1", "--2"], vec!["--1", "--2"]; "two short ids")]
    #[test_case(vec!["-------11111111111111111111111111", "-------22222222222222222222222222"], vec!["-------1", "-------2"]; "two long ids resulting in 8 char")]
    #[test_case(vec!["-------------------------------11", "-------------------------------22"], vec!["-------------------------------1", "-------------------------------2"]; "two long ids resulting in 32 char")]
    fn test(input: Vec<&str>, expected_output: Vec<&str>) {
        let input: Vec<String> = input.into_iter().map(String::from).collect();
        let output = shorten_cert_ids(&input);

        assert_eq!(output, expected_output);
    }
}
