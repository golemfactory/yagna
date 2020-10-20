mod service;

pub struct SgxService;

impl SgxService {
    pub async fn gsb<Context>(_context: &Context) -> anyhow::Result<()> {
        self::service::bind_gsb();
        Ok(())
    }
}
