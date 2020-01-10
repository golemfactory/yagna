use crate::dao::AppKeyDao;
use crate::error::Error;
use futures::lock::Mutex;
use std::sync::Arc;
use uuid::Uuid;
use ya_core_model::appkey as model;
use ya_persistence::executor::DbExecutor;

macro_rules! bind_gsb_method {
    ($db_executor:expr, $method:ident) => {{
        use ya_core_model::appkey as model;
        use ya_service_bus::typed as bus;

        let cloned_db = $db_executor.clone();
        let _ = bus::bind(&model::APP_KEY_SERVICE_ID, move |m| {
            $method(cloned_db.clone(), m)
        });
    }};
}

pub fn bind_gsb(db: Arc<Mutex<DbExecutor<Error>>>) {
    log::info!("activating appkey service");
    bind_gsb_method!(db, create);
    bind_gsb_method!(db, get);
    bind_gsb_method!(db, list);
    bind_gsb_method!(db, remove);
    log::info!("appkey service activated");
}

macro_rules! db_conn {
    ($db_executor:expr) => {
        $db_executor.lock().await.conn().map_err(Into::into)
    };
}

/// Create a new application key entry
async fn create(
    db: Arc<Mutex<DbExecutor<Error>>>,
    create: model::Create,
) -> Result<(), model::Error> {
    let conn = db_conn!(db)?;
    let dao = AppKeyDao::new(&conn);
    let key = Uuid::new_v4().to_simple().to_string();
    dao.create(key, create.name, create.role, create.identity)
        .map_err(Into::into)?;

    Ok(())
}

/// Retrieve an application key entry based on the key itself
async fn get(
    db: Arc<Mutex<DbExecutor<Error>>>,
    get: model::Get,
) -> Result<model::AppKey, model::Error> {
    let conn = db_conn!(db)?;
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

/// List available application key entries
async fn list(
    db: Arc<Mutex<DbExecutor<Error>>>,
    list: model::List,
) -> Result<(Vec<model::AppKey>, u32), model::Error> {
    let conn = db_conn!(db)?;
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

/// Remove an application key
async fn remove(
    db: Arc<Mutex<DbExecutor<Error>>>,
    remove: model::Remove,
) -> Result<(), model::Error> {
    let conn = db_conn!(db)?;
    let dao = AppKeyDao::new(&conn);
    dao.remove(remove.name, remove.identity)
        .map_err(Into::into)?;

    Ok(())
}
