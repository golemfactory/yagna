use crate::db::dao::{AgreementDao, DemandDao, OfferDao, ProposalDao};
use actix::prelude::*;
use futures::join;
use std::time::Duration;
use ya_persistence::executor::DbExecutor;

async fn clean(db: DbExecutor) {
    let demand_db = db.clone();
    let offer_db = db.clone();
    let agreement_db = db.clone();
    let proposal_db = db.clone();

    let results = join!(
        async move { demand_db.as_dao::<DemandDao>().clean().await },
        async move { offer_db.as_dao::<OfferDao>().clean().await },
        async move { agreement_db.as_dao::<AgreementDao>().clean().await },
        async move { proposal_db.as_dao::<ProposalDao>().clean().await },
        // async move { events_db.as_dao::<EventsDao>().clean().await },
    );
    let v_results = vec![
        results.0, results.1, results.2, results.3,
        // results.4,
    ];
    for db_result in v_results.into_iter() {
        match db_result {
            Err(e) => log::error!("Market database cleaner error: {}", e),
            _ => (),
        }
    }
}

struct DatabaseCleaner {
    db: DbExecutor,
}

impl DatabaseCleaner {
    pub fn new(db: DbExecutor) -> Addr<Self> {
        let dc = Self { db };
        dc.start()
    }
}

impl Actor for DatabaseCleaner {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_job(ctx);
    }
}

impl DatabaseCleaner {
    fn start_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(3600), |act, _ctx| {
            log::debug!("Market database cleaner job started");
            let db = act.db.clone();
            Arbiter::spawn(async move {
                clean(db).await;
            });
            log::debug!("Market database cleaner job done");
        });
    }
}
