use super::Keystore;

pub(super) trait GolemCertAddParams {}

struct GolemKeystore {}

impl Keystore for GolemKeystore {
    fn add(&mut self, _add: &super::AddParams) -> anyhow::Result<super::AddResponse> {
        Ok(Default::default())
    }

    fn remove(&mut self, _remove: &super::RemoveParams) -> anyhow::Result<super::RemoveResponse> {
        Ok(Default::default())
    }

    fn list(&self) -> Vec<super::CertData> {
        Default::default()
    }
}
