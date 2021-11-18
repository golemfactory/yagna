#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum KeystoreConfig {
    /// List trusted keys
    List,
    /// Create a new profile
    Add {
        scheme: String,
        key: String,
        name: Option<String>,
    },
    /// Update a profile
    Remove { name: String },
}
