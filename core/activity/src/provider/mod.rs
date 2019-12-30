use crate::common::{generate_id, PathActivity, QueryTimeoutMaxCount, RpcMessageResult};
use crate::dao::{ActivityDao, AgreementDao, EventDao, InnerIntoOption};
use crate::db::{ConnType, DbExecutor};
use crate::error::Error;
use crate::timeout::IntoTimeoutFuture;
use crate::{GsbApi, RestfulApi, ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION};
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use ya_core_model::activity::*;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent, State};
use ya_service_bus::typed as bus;

pub struct ProviderActivityApi {
    db_executor: Mutex<DbExecutor>,
}

impl ProviderActivityApi {
    pub fn new(db_executor: Mutex<DbExecutor>) -> Self {
        Self { db_executor }
    }

    #[inline(always)]
    async fn conn(&self) -> Result<ConnType, Error> {
        self.db_executor.lock().await.conn()
    }
}

impl ProviderActivityApi {
    /// Creates new Activity based on given Agreement.
    async fn create_activity(&self, msg: CreateActivity) -> RpcMessageResult<CreateActivity> {
        let conn = self.conn().await?;
        let activity_id = generate_id();

        // Check whether agreement exists
        AgreementDao::new(&conn)
            .get(&msg.agreement_id)
            .map_err(Error::from)?;

        ActivityDao::new(&conn)
            .create(&activity_id, &msg.agreement_id, None, None)
            .map_err(Error::from)?;

        EventDao::new(&conn)
            .create(&ProviderEvent::CreateActivity {
                activity_id: activity_id.clone(),
                agreement_id: msg.agreement_id,
            })
            .map_err(Error::from)?;

        ActivityDao::new(&conn)
            .get_state_fut(&activity_id, None)
            .timeout(msg.timeout)
            .map_err(Error::from)
            .await
            .map_err(Error::from)?;

        Ok(activity_id)
    }

    /// Destroys given Activity.
    async fn destroy_activity(&self, msg: DestroyActivity) -> RpcMessageResult<DestroyActivity> {
        let conn = self.conn().await?;

        EventDao::new(&conn)
            .create(&ProviderEvent::DestroyActivity {
                activity_id: msg.activity_id.clone(),
                agreement_id: msg.agreement_id,
            })
            .map_err(Error::from)?;

        ActivityDao::new(&conn)
            .get_state_fut(&msg.activity_id, Some(State::Terminated))
            .timeout(msg.timeout)
            .map_err(Error::from)
            .await?;

        Ok(())
    }

    /// Get state of specified Activity.
    async fn get_activity_state(
        &self,
        msg: GetActivityState,
    ) -> RpcMessageResult<GetActivityState> {
        ActivityDao::new(&self.conn().await?)
            .get_state(&msg.activity_id)
            .inner_into_option()
            .map_err(Error::from)?
            .ok_or(Error::NotFound.into())
    }

    /// Get usage of specified Activity.
    async fn get_activity_usage(
        &self,
        msg: GetActivityUsage,
    ) -> RpcMessageResult<GetActivityUsage> {
        ActivityDao::new(&self.conn().await?)
            .get_usage(&msg.activity_id)
            .inner_into_option()
            .map_err(Error::from)?
            .ok_or(Error::NotFound.into())
    }
}

impl ProviderActivityApi {
    /// Fetch Requestor command events.
    async fn get_events(
        &self,
        query: web::Query<QueryTimeoutMaxCount>,
    ) -> Result<Vec<ProviderEvent>, Error> {
        EventDao::new(&self.conn().await?)
            .get_events_fut(query.max_count)
            .timeout(query.timeout)
            .map_err(Error::from)
            .await
            .map_err(Error::from)
    }

    /// Pass activity state (which may include error details).
    async fn set_activity_state(
        &self,
        path: web::Path<PathActivity>,
        activity_state: web::Json<ActivityState>,
    ) -> Result<(), Error> {
        ActivityDao::new(&self.conn().await?)
            .set_state(&path.activity_id, &activity_state)
            .map_err(Error::from)
    }

    /// Pass current activity usage (which may include error details).
    async fn set_activity_usage(
        &self,
        path: web::Path<PathActivity>,
        activity_usage: web::Json<ActivityUsage>,
    ) -> Result<(), Error> {
        ActivityDao::new(&self.conn().await?)
            .set_usage(&path.activity_id, &activity_usage)
            .map_err(Error::from)
    }
}

impl GsbApi for ProviderActivityApi {
    fn bind(instance: &'static Self) {
        let _ = bus::bind(
            &format!("/{}/create_activity", ACTIVITY_SERVICE_ID),
            move |m| instance.create_activity(m),
        );
        let _ = bus::bind(
            &format!("/{}/destroy_activity", ACTIVITY_SERVICE_ID),
            move |m| instance.destroy_activity(m),
        );
        let _ = bus::bind(
            &format!("/{}/get_activity_state", ACTIVITY_SERVICE_ID),
            move |m| instance.get_activity_state(m),
        );
        let _ = bus::bind(
            &format!("/{}/get_activity_usage", ACTIVITY_SERVICE_ID),
            move |m| instance.get_activity_usage(m),
        );
    }
}

impl RestfulApi for ProviderActivityApi {
    fn web_scope(api: &'static Self) -> actix_web::Scope {
        web::scope(&format!(
            "/{}/v{}",
            ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION
        ))
        .service(
            web::resource("/events")
                .route(web::get().to_async(impl_restful_handler!(api, get_events, query))),
        )
        .service(
            web::resource("/activity/{activity_id}/state").route(
                web::put().to_async(impl_restful_handler!(api, set_activity_state, path, body)),
            ),
        )
        .service(
            web::resource("/activity/{activity_id}/usage").route(
                web::put().to_async(impl_restful_handler!(api, set_activity_usage, path, body)),
            ),
        )
    }
}
