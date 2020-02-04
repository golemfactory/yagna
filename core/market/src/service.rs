use std::convert::TryInto;

use ya_client::{market::MarketProviderApi, web::WebClient};
use ya_core_model::{appkey, market as model};
use ya_model::market::Agreement;
use ya_persistence::executor::DbExecutor;
use ya_service_api::constants::NET_SERVICE_ID;
use ya_service_bus::{actix_rpc, RpcMessage};

use crate::{dao, error::Error};

pub type RpcMessageResult<T> = Result<<T as RpcMessage>::Item, <T as RpcMessage>::Error>;

pub fn activate(db: &DbExecutor) {
    let _ = dao::init(db);
    bind_gsb_method!(model::BUS_ID, db, get_agreement);
}

pub async fn get_agreement(
    db: DbExecutor,
    caller: String,
    msg: model::GetAgreement,
) -> RpcMessageResult<model::GetAgreement> {
    let conn = db.conn().map_err(Error::from)?;

    let agreement_dao = dao::AgreementDao::new(&conn);

    if let Ok(agreement) = agreement_dao.get(&msg.agreement_id) {
        return Ok(agreement.try_into().map_err(Error::from)?);
    }

    let agreement = fetch_agreement(caller, &msg.agreement_id)
        .await
        .map_err(Error::from)?;
    log::info!("inserting agreement: {:#?}", agreement);

    agreement_dao
        .create(agreement.clone().try_into().map_err(Error::from)?)
        .map_err(Error::from)?;
    log::info!("agreement inserted");

    Ok(agreement)
}

pub(crate) async fn fetch_agreement(
    caller: String,
    agreement_id: &String,
) -> Result<Agreement, Error> {
    // FIXME: move this logic to ya_net or event do not pass /net prefix
    let caller_id = caller.replacen(NET_SERVICE_ID, "", 1).replacen("/", "", 1);
    log::info!("fetching appkey for caller: {}", caller_id);
    let app_key = match actix_rpc::service(appkey::BUS_ID)
        .send(None, appkey::Get::with_identity(caller_id))
        .await?
    {
        Ok(key) => key,
        Err(_) => {
            log::info!("getting appkey for default id");
            actix_rpc::service(appkey::BUS_ID)
                .send(None, appkey::Get::default())
                .await??
        }
    };
    log::info!("using appkey: {:?}", app_key);
    let market_api: MarketProviderApi = WebClient::with_token(&app_key.key)?.interface()?;
    log::info!("fetching agreement");
    Ok(market_api.get_agreement(agreement_id).await?)
}
