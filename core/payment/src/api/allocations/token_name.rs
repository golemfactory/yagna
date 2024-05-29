use std::fmt::Display;

use ya_core_model::payment::local::{get_token_from_network_name, DriverName, NetworkName};

pub struct TokenName(String);

impl Display for TokenName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TokenName {
    pub fn default(_driver: &DriverName, network: &NetworkName) -> TokenName {
        Self(get_token_from_network_name(network).to_lowercase())
    }

    pub fn from_token_string(
        driver: &DriverName,
        network: &NetworkName,
        token: &str,
    ) -> Result<Self, String> {
        if token == "GLM" || token == "tGLM" {
            return Err(format!(
                "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
                token
            ));
        }
        let token_expected = Self::default(driver, network).to_string();
        if token != token_expected {
            return Err(format!(
                    "Token {} does not match expected token {} for driver {} and network {}. \
            Note that for test networks expected token name is tglm and for production networks it is glm",
                    token, token_expected, driver, network
                ));
        }
        Ok(Self(token.to_string()))
    }
}
