use std::path::PathBuf;
use structopt::*;

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum IdentityCommand {
    /// Show list of all identities
    List,

    /// Display identity
    Show {
        /// Identity alias to show
        alias: Option<String>,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        #[structopt(long)]
        alias: Option<String>,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,
    },
    /// Update given identity
    Update {
        /// Identity alias to update
        #[structopt(long)]
        alias: Option<String>,
    },

    /// Drop given identity
    Drop {
        /// Identity alias to drop
        alias: String,
    },
}

#[cfg(test)]
mod tests {
    use ethkey::prelude::*;

    #[test]
    fn test_ethkey() {
        let key = EthAccount::load_or_generate("/tmp/path/to/keystore", "passwd")
            .expect("should load or generate new eth key");

        println!("{:?}", key.address());

        let message = [7_u8; 32];

        // sign the message
        let signature = key.sign(&message).unwrap();

        // verify the signature
        let result = key.verify(&signature, &message).unwrap();
        println!(
            "{}",
            if result {
                "verification ok"
            } else {
                "wrong signature"
            }
        );
    }
}
