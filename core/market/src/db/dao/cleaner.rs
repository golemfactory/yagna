use futures::join;
use tokio::time;

use crate::config::DbConfig;
use crate::db::dao::{AgreementDao, DemandDao, NegotiationEventsDao, OfferDao, ProposalDao};
use crate::db::DbMixedExecutor;

pub async fn clean(db: DbMixedExecutor, cfg: &DbConfig) {
    let demand_db = db.clone();
    let events_db = db.clone();
    let offer_db = db.clone();
    let agreement_db = db.clone();
    let proposal_db = db.clone();

    let results = join!(
        async move { demand_db.as_dao::<DemandDao>().clean().await },
        async move { offer_db.as_dao::<OfferDao>().clean().await },
        async move { agreement_db.as_dao::<AgreementDao>().clean(cfg).await },
        async move { proposal_db.as_dao::<ProposalDao>().clean().await },
        async move { events_db.as_dao::<NegotiationEventsDao>().clean(cfg).await },
    );
    let v_results = vec![results.0, results.1, results.2, results.3, results.4];
    for db_result in v_results.into_iter() {
        match db_result {
            Err(e) => log::error!("Market database cleaner error: {}", e),
            _ => (),
        }
    }
}

pub async fn clean_forever(db: DbMixedExecutor, cfg: DbConfig) {
    let mut interval = time::interval(cfg.cleanup_interval);
    loop {
        interval.tick().await;
        log::debug!("Market database cleaner job started");
        let db = db.clone();
        clean(db, &cfg).await;
        log::debug!("Market database cleaner job done");
    }
}
