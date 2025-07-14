use anyhow::anyhow;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;
use ya_agreement_utils::agreement::{expand, flatten};

use ya_client::model::market::{agreement::State, Role};
use ya_client::model::NodeId;
use ya_core_model::market::local as market_bus;
use ya_core_model::market::{
    FundGolemBase, GetAgreement, GetGolemBaseBalance, GetGolemBaseOffer, GolemBaseCommand,
    GolemBaseCommandType, ListAgreements,
};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

/// Market management
#[derive(StructOpt, Debug)]
pub enum Command {
    Agreements(AgreementsCommand),
    GolemBase(GolemBaseCliCommand),
    Offer(OfferCommand),
}

impl Command {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            Command::Agreements(agreements_cmd) => agreements_cmd.run_command(ctx).await,
            Command::GolemBase(golembase_cmd) => golembase_cmd.run_command(ctx).await,
            Command::Offer(offer_cmd) => offer_cmd.run_command(ctx).await,
        }
    }
}

#[derive(StructOpt, Debug)]
pub enum AgreementsCommand {
    List {
        #[structopt(long, help = "Only show agreements with this state")]
        state: Option<State>,
        #[structopt(long, help = "Only show agreements after this date, rfc3339")]
        before: Option<DateTime<Utc>>,
        #[structopt(long, help = "Only show agreements before this date, rfc3339")]
        after: Option<DateTime<Utc>>,
        #[structopt(long, help = "Only show agreements with this app session id")]
        app_session_id: Option<String>,
    },
    Get {
        #[structopt(long, help = "Agreement ID, may be obtained via list-agreements")]
        id: String,
        #[structopt(long, help = "Your role in the agreement (Provider | Requestor)")]
        role: Role,
    },
}

impl AgreementsCommand {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            AgreementsCommand::List {
                state,
                before,
                after,
                app_session_id,
            } => {
                let request = ListAgreements {
                    state,
                    before_date: before,
                    after_date: after,
                    app_session_id,
                };

                let agreements = bus::service(market_bus::BUS_ID).send(request).await??;

                let mut agreements_json = Vec::new();
                for agreement in agreements {
                    agreements_json.push(serde_json::to_value([
                        agreement.id,
                        agreement.role.to_string(),
                        agreement.timestamp.to_rfc3339(),
                        agreement
                            .approved_date
                            .map(|ts| ts.to_rfc3339())
                            .unwrap_or_else(|| "N/A".to_owned()),
                    ])?);
                }

                Ok(ResponseTable {
                    columns: vec![
                        "id".to_owned(),
                        "role".to_owned(),
                        "created".to_owned(),
                        "approved".to_owned(),
                    ],
                    values: agreements_json,
                }
                .with_header("\nMatching agreements:\n".to_owned()))
            }
            AgreementsCommand::Get { id, role } => {
                let request = GetAgreement {
                    agreement_id: id,
                    role,
                };

                let agreement = bus::service(market_bus::BUS_ID).send(request).await??;

                CommandOutput::object(agreement)
            }
        }
    }
}

#[derive(StructOpt, Debug)]
pub enum GolemBaseCliCommand {
    /// Fund GolemBase wallet
    Fund {
        #[structopt(
            help = "Wallet address to fund (optional, uses default identity if not provided)"
        )]
        wallet: Option<NodeId>,
    },
    /// Check GolemBase wallet balance
    Balance {
        #[structopt(
            help = "Wallet address to check (optional, uses default identity if not provided)"
        )]
        wallet: Option<NodeId>,
    },
    /// Get offer from GolemBase
    GetOffer {
        #[structopt(help = "Offer ID to retrieve")]
        offer_id: String,
        #[structopt(long, help = "Flatten offer")]
        flatten: bool,
    },
    /// Get transaction from GolemBase
    GetTransaction {
        #[structopt(help = "Transaction ID to retrieve")]
        transaction_id: String,
    },
    /// Get block from GolemBase
    GetBlock {
        #[structopt(help = "Block number to retrieve")]
        block_number: u64,
    },
}

impl GolemBaseCliCommand {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            GolemBaseCliCommand::Fund { wallet } => {
                let request = FundGolemBase { wallet };
                let response = bus::service(market_bus::discovery_endpoint())
                    .send(request)
                    .await??;

                CommandOutput::object(json!({
                    "message": format!("GolemBase wallet {} funded, balance {} tGLM", response.wallet, response.balance)
                }))
            }
            GolemBaseCliCommand::Balance { wallet } => {
                let request = GetGolemBaseBalance { wallet };
                let response = bus::service(market_bus::discovery_endpoint())
                    .send(request)
                    .await??;

                CommandOutput::object(json!({
                    "message": format!("GolemBase wallet {} balance: {} tGLM", response.wallet, response.balance),
                    "balance": response.balance
                }))
            }
            GolemBaseCliCommand::GetOffer { offer_id, flatten } => {
                let request = GetGolemBaseOffer { offer_id };
                let response = bus::service(market_bus::discovery_endpoint())
                    .send(request)
                    .await??;

                let mut offer = response.offer;
                offer.properties = if flatten {
                    serde_json::to_value(ya_agreement_utils::agreement::flatten(offer.properties))?
                } else {
                    offer.properties
                };

                CommandOutput::object(json!({
                    "offer": offer,
                    "currentBlock": response.current_block,
                    "metadata": response.metadata
                }))
            }
            GolemBaseCliCommand::GetTransaction { transaction_id } => {
                let request = GolemBaseCommand {
                    command: GolemBaseCommandType::GetTransaction { transaction_id },
                };
                let response = bus::service(market_bus::discovery_endpoint())
                    .send(request)
                    .await??;

                CommandOutput::object(response.response)
            }
            GolemBaseCliCommand::GetBlock { block_number } => {
                let request = GolemBaseCommand {
                    command: GolemBaseCommandType::GetBlock { block_number },
                };
                let response = bus::service(market_bus::discovery_endpoint())
                    .send(request)
                    .await??;

                CommandOutput::object(response.response)
            }
        }
    }
}

#[derive(StructOpt, Debug)]
pub enum OfferCommand {
    /// Format offer in flat structure
    FormatFlat {
        #[structopt(help = "Path to the offer file")]
        file: PathBuf,
    },
    /// Format offer in expanded structure
    FormatExpanded {
        #[structopt(help = "Path to the offer file")]
        file: PathBuf,
    },
}

impl OfferCommand {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        let path = match &self {
            OfferCommand::FormatFlat { file } | OfferCommand::FormatExpanded { file } => file,
        };

        let content = fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read file {}: {}", path.display(), e))?;
        let json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow!("Failed to parse JSON from file: {}", e))?;

        match self {
            OfferCommand::FormatFlat { .. } => CommandOutput::object(flatten(json)),
            OfferCommand::FormatExpanded { .. } => CommandOutput::object(expand(json)),
        }
    }
}
