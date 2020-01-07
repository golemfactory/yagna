use crate::dao::AppKeyDao;
use crate::error::Error;
use futures::lock::Mutex;
use uuid::Uuid;
use ya_core_model::appkey as model;
use ya_persistence::executor::{ConnType, DbExecutor};
use ya_service_bus::typed as bus;

pub fn bind(service: &'static AppKeyService) {
    let _ = bus::bind(model::ID, move |c| service.create(c));
    let _ = bus::bind(model::ID, move |g| service.get(g));
    let _ = bus::bind(model::ID, move |l| service.list(l));
    let _ = bus::bind(model::ID, move |r| service.remove(r));
}

pub struct AppKeyService {
    db_executor: Mutex<DbExecutor<Error>>,
}

impl AppKeyService {
    pub fn new(db_executor: Mutex<DbExecutor<Error>>) -> Self {
        AppKeyService { db_executor }
    }

    #[inline(always)]
    async fn conn(&self) -> Result<ConnType, Error> {
        self.db_executor.lock().await.conn()
    }
}

impl AppKeyService {
    async fn create(&self, create: model::Create) -> Result<(), model::Error> {
        let conn = self.conn().await.map_err(Into::into)?;
        let dao = AppKeyDao::new(&conn);
        let key = Uuid::new_v4().to_simple().to_string();
        dao.create(key, create.name, create.role, create.identity)
            .map_err(Into::into)?;

        Ok(())
    }

    async fn get(&self, get: model::Get) -> Result<model::AppKey, model::Error> {
        let conn = self.conn().await.map_err(Into::into)?;
        let dao = AppKeyDao::new(&conn);
        let result = dao.get(get.key).map_err(Into::into)?;

        Ok(model::AppKey {
            name: result.0.name,
            key: result.0.key,
            role: result.1.name,
            identity: result.0.identity,
            created_date: result.0.created_date,
        })
    }

    async fn list(&self, list: model::List) -> Result<(Vec<model::AppKey>, u32), model::Error> {
        let conn = self.conn().await.map_err(Into::into)?;
        let dao = AppKeyDao::new(&conn);
        let result = dao
            .list(list.identity, list.page, list.per_page)
            .map_err(Into::into)?;

        let keys = result
            .0
            .into_iter()
            .map(|(app_key, role)| model::AppKey {
                name: app_key.name,
                key: app_key.key,
                role: role.name,
                identity: app_key.identity,
                created_date: app_key.created_date,
            })
            .collect();

        Ok((keys, result.1))
    }

    async fn remove(&self, remove: model::Remove) -> Result<(), model::Error> {
        let conn = self.conn().await.map_err(Into::into)?;
        let dao = AppKeyDao::new(&conn);
        dao.remove(remove.name, remove.identity)
            .map_err(Into::into)?;

        Ok(())
    }
}
