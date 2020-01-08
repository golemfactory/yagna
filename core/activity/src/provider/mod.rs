use crate::common::{generate_id, PathActivity, QueryTimeoutMaxCount, RpcMessageResult};
use crate::dao::{
    ActivityDao, ActivityStateDao, ActivityUsageDao, AgreementDao, Event, EventDao,
    NotFoundAsOption,
};
use crate::error::Error;
use crate::timeout::IntoTimeoutFuture;
use crate::{GsbApi, RestfulApi, ACTIVITY_SERVICE_ID, ACTIVITY_SERVICE_VERSION};
use actix_web::web;
use futures::lock::Mutex;
use futures::prelude::*;
use std::convert::From;
use ya_core_model::activity::*;
use ya_model::activity::provider_event::ProviderEventType;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent, State};
use ya_persistence::executor::{ConnType, DbExecutor};
use ya_service_bus::typed as bus;

impl From<Event> for ProviderEvent {
    fn from(value: Event) -> Self {
        let event_type = serde_json::from_str::<ProviderEventType>(&value.name).unwrap();
        match event_type {
            ProviderEventType::CreateActivity => ProviderEvent::CreateActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
            ProviderEventType::DestroyActivity => ProviderEvent::DestroyActivity {
                activity_id: value.activity_natural_id,
                agreement_id: value.agreement_natural_id,
            },
        }
    }
}

pub struct ProviderActivityApi {
    db_executor: Mutex<DbExecutor<Error>>,
}

impl ProviderActivityApi {
    pub fn new(db_executor: Mutex<DbExecutor<Error>>) -> Self {
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
            .create(&activity_id, &msg.agreement_id)
            .map_err(Error::from)?;

        EventDao::new(&conn)
            .create(
                &activity_id,
                serde_json::to_string(&ProviderEventType::CreateActivity)
                    .unwrap()
                    .as_str(),
            )
            .map_err(Error::from)?;

        ActivityStateDao::new(&conn)
            .get_future(&activity_id, None)
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
            .create(
                &msg.activity_id,
                serde_json::to_string(&ProviderEventType::DestroyActivity)
                    .unwrap()
                    .as_str(),
            )
            .map_err(Error::from)?;

        ActivityStateDao::new(&conn)
            .get_future(&msg.activity_id, Some(State::Terminated))
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
        ActivityStateDao::new(&self.conn().await?)
            .get(&msg.activity_id)
            .not_found_as_option()
            .map_err(Error::from)?
            .map(|state| ActivityState {
                state: serde_json::from_str(&state.name).unwrap(),
                reason: state.reason,
                error_message: state.error_message,
            })
            .ok_or(Error::NotFound.into())
    }

    /// Get usage of specified Activity.
    async fn get_activity_usage(
        &self,
        msg: GetActivityUsage,
    ) -> RpcMessageResult<GetActivityUsage> {
        ActivityUsageDao::new(&self.conn().await?)
            .get(&msg.activity_id)
            .not_found_as_option()
            .map_err(Error::from)?
            .map(|usage| ActivityUsage {
                current_usage: usage
                    .vector_json
                    .map(|json| serde_json::from_str(&json).unwrap()),
            })
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
            .map(|events| {
                events
                    .into_iter()
                    .map(|event| ProviderEvent::from(event))
                    .collect()
            })
    }

    /// Pass activity state (which may include error details).
    async fn set_activity_state(
        &self,
        path: web::Path<PathActivity>,
        activity_state: web::Json<ActivityState>,
    ) -> Result<(), Error> {
        ActivityStateDao::new(&self.conn().await?)
            .set(
                &path.activity_id,
                activity_state.state.clone(),
                activity_state.reason.clone(),
                activity_state.error_message.clone(),
            )
            .map_err(Error::from)
    }

    /// Pass current activity usage (which may include error details).
    async fn set_activity_usage(
        &self,
        path: web::Path<PathActivity>,
        activity_usage: web::Json<ActivityUsage>,
    ) -> Result<(), Error> {
        ActivityUsageDao::new(&self.conn().await?)
            .set(&path.activity_id, &activity_usage.current_usage)
            .map_err(Error::from)
    }
}

impl GsbApi for ProviderActivityApi {
    fn bind(instance: &'static Self) {
        let _ = bus::bind(&ACTIVITY_SERVICE_ID, move |m| instance.create_activity(m));
        let _ = bus::bind(&ACTIVITY_SERVICE_ID, move |m| instance.destroy_activity(m));
        let _ = bus::bind(&ACTIVITY_SERVICE_ID, move |m| {
            instance.get_activity_state(m)
        });
        let _ = bus::bind(&ACTIVITY_SERVICE_ID, move |m| {
            instance.get_activity_usage(m)
        });
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
                .route(web::get().to(impl_restful_handler!(api, get_events, query))),
        )
        .service(
            web::resource("/activity/{activity_id}/state")
                .route(web::put().to(impl_restful_handler!(api, set_activity_state, path, body))),
        )
        .service(
            web::resource("/activity/{activity_id}/usage")
                .route(web::put().to(impl_restful_handler!(api, set_activity_usage, path, body))),
        )
    }
}
