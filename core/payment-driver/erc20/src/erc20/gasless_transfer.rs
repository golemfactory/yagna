use core::fmt;
use std::{env, fmt::Display};

use awc::http;
use ethereum_types::{H160, H256};
use serde::{Deserialize, Serialize};
use ya_payment_driver::model::GenericError;
use ya_utils_networking::resolver;

use crate::erc20::{
    eth_utils::keccak256_hash,
    ethereum,
    utils::{big_dec_to_u256, str_to_addr},
};

const DEFAULT_GASLESS_HOST: &str = "http://gasless.golem.network";
const GASLESS_ADDR_ENVAR: &str = "GASLESS_SERVER_ADDRESS";
const TRANSFER_ENDPOINT: &str = "/api/forward/transfer";

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GaslessRequest {
    v: String,
    r: H256,
    s: H256,
    sender: H160,
    abi_function_call: String,
}

impl Display for GaslessRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_json::to_string_pretty(&self).unwrap())
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GaslessResponse {
    tx_id: H256,
}

#[derive(Deserialize, Debug)]
struct ErrorResponse {
    message: String,
}

async fn create_gasless_request(
    details: &ya_payment_driver::model::PaymentDetails,
    network: ya_payment_driver::db::models::Network,
) -> Result<GaslessRequest, GenericError> {
    let sender = str_to_addr(&details.sender)?;
    let recipient = str_to_addr(&details.recipient)?;
    let amount = big_dec_to_u256(&details.amount)?;

    let nonce = ethereum::get_nonce_from_contract(sender, network).await?;
    let transfer_abi = ethereum::encode_transfer_abi(recipient, amount, network).await?;

    let meta_transfer = ethereum::encode_meta_transaction_to_eip712(
        sender,
        recipient,
        amount,
        nonce,
        &transfer_abi,
        network,
    )
    .await?;

    let hash_of_meta_transfer = keccak256_hash(&meta_transfer);

    let mut signature = ethereum::sign_hash_of_data(sender, hash_of_meta_transfer).await?;

    const ETH_V_OFFSET: u8 = 27;

    signature[0] += ETH_V_OFFSET;

    let request = GaslessRequest {
        v: format!("0x{:02x}", signature[0]),
        r: H256::from_slice(&signature[1..33]),
        s: H256::from_slice(&signature[33..]),
        sender,
        abi_function_call: format!("0x{}", hex::encode(&transfer_abi)),
    };

    debug!("signature: {signature:02X?}");

    Ok(request)
}

pub async fn send_gasless_transfer(
    details: &ya_payment_driver::model::PaymentDetails,
    network: ya_payment_driver::db::models::Network,
) -> Result<H256, GenericError> {
    let request = create_gasless_request(details, network).await?;

    info!("Sending request to the gasless forwarder: {}...", request);

    let http_client = awc::Client::new();

    let request_url = format!("{}{TRANSFER_ENDPOINT}", resolve_gasless_url());
    let request_url = resolver::try_resolve_dns_record(&request_url).await;

    debug!("Sending request to {request_url}");

    let mut resp = http_client
        .post(request_url)
        .send_json(&request)
        .await
        .map_err(|e| {
            GenericError::new(format!(
                "While sending a request to the gasless server: {e}"
            ))
        })?;

    info!("Got response, status {}", resp.status());

    match resp.status() {
        http::StatusCode::OK => {
            let resp_body: GaslessResponse = resp
                .json()
                .await
                .map_err(|e| GenericError::new(format!("While parsing response body: {e}")))?;

            Ok(resp_body.tx_id)
        }

        http::StatusCode::TOO_MANY_REQUESTS => {
            let retry_after = match resp.headers().get(http::header::RETRY_AFTER) {
                Some(retry) => retry
                    .to_str()
                    .map_err(|e| {
                        GenericError::new(format!(
                            "Too many requests, while converting header to string: {e}"
                        ))
                    })?
                    .to_string(),
                None => "not specified".to_string(),
            };

            let err: ErrorResponse = resp.json().await.map_err(|e| {
                GenericError::new(format!("Too many requests, while parsing error: {e}"))
            })?;

            Err(GenericError::new(format!(
                "{}, retry after: {}",
                err.message, retry_after
            )))
        }

        http::StatusCode::BAD_REQUEST => {
            let err: ErrorResponse = resp
                .json()
                .await
                .map_err(|e| GenericError::new(format!("Bad request, while parsing error: {e}")))?;

            Err(GenericError::new(err.message))
        }

        status => {
            let resp_bytes = resp.body().await.map_err(GenericError::new)?;

            Err(GenericError::new(format!(
                "Invalid gasless forwarder response, status code: {status}, body {}",
                String::from_utf8_lossy(resp_bytes.as_ref())
            )))
        }
    }
}

fn resolve_gasless_url() -> String {
    env::var(GASLESS_ADDR_ENVAR).unwrap_or_else(|_| DEFAULT_GASLESS_HOST.to_string())
}
