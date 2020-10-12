use crate::error::Error;
use crate::message::*;
use crate::runtime::Runtime;
use crate::service::ServiceAddr;
use crate::state::State;
use crate::{report, ExeUnit};
use actix::prelude::*;
use futures::FutureExt;
use ya_client_model::activity;
use ya_core_model::activity::local::SetState as SetActivityState;

impl<R: Runtime> Handler<GetState> for ExeUnit<R> {
    type Result = <GetState as Message>::Result;

    fn handle(&mut self, _: GetState, _: &mut Context<Self>) -> Self::Result {
        GetStateResponse(self.state.inner)
    }
}

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    type Result = <SetState as Message>::Result;

    fn handle(&mut self, msg: SetState, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(update) = msg.running_command {
            self.state.running_command = update.cmd;
        }

        if let Some(update) = msg.batch_result {
            self.state.push_batch_result(update.batch_id, update.result);
        }

        if let Some(update) = msg.state {
            if self.state.inner != update.state {
                log::debug!("Entering state: {:?}", update.state);
                log::debug!("Report: {}", self.state.report());

                self.state.inner = update.state.clone();

                if let Some(id) = &self.ctx.activity_id {
                    let credentials = match &update.state {
                        activity::StatePair(activity::State::Initialized, None) => {
                            self.ctx.credentials.clone()
                        }
                        _ => None,
                    };
                    let fut = report(
                        self.ctx.report_url.clone().unwrap(),
                        SetActivityState::new(
                            id.clone(),
                            activity::ActivityState {
                                state: update.state,
                                reason: update.reason,
                                error_message: None,
                            },
                            credentials,
                        ),
                    );
                    ctx.spawn(fut.into_actor(self));
                }
            }
        }
    }
}

impl<R: Runtime> Handler<Initialize> for ExeUnit<R> {
    type Result = ResponseActFuture<Self, <Initialize as Message>::Result>;

    #[cfg(feature = "sgx")]
    fn handle(&mut self, _: Initialize, _: &mut Context<Self>) -> Self::Result {
        let crypto = self.ctx.crypto.clone();
        let nonce = self.ctx.activity_id.to_owned();
        let task_package = self.ctx.agreement.task_package.to_owned();

        let fut = async move {
            Ok::<_, Error>({
                {
                    use graphene_sgx::sgx::SgxQuote;
                    use sha3::{Digest, Sha3_256};
                    use std::env;
                    use ya_client_model::node_id::{NodeId, ParseError};
                    use ya_core_model::activity::local::Credentials;
                    use ya_core_model::net::RemoteEndpoint;
                    use ya_core_model::sgx::VerifyAttestationEvidence;

                    let quote = SgxQuote::hasher()
                        .data(&crypto.requestor_pub_key.serialize())
                        .data(&crypto.pub_key.serialize())
                        .data(task_package.as_bytes())
                        .build()?;

                    let mr_enclave = quote.body.report_body.mr_enclave;
                    log::debug!("Enclave quote: {:?}", &quote);

                    let remote: NodeId = env::var("IAS_SERVICE_ADDRESS")
                        .map_err(|_| {
                            Error::Attestation("IAS_SERVICE_ADDRESS variable not found".into())
                        })?
                        .parse()
                        .map_err(|e: ParseError| Error::Attestation(e.to_string()))?;

                    let evidence = remote
                        .service("/public/sgx")
                        .call(VerifyAttestationEvidence {
                            enclave_quote: quote.into(),
                            ias_nonce: nonce,
                            production: false,
                        })
                        .await?
                        .map_err(|e| Error::Attestation(e.to_string()))?;

                    log::debug!("IAS report: {}", &evidence.report);
                    let mut hasher = Sha3_256::new();
                    hasher.input(task_package.as_bytes());

                    let mut payload_hash = [0u8; 32];
                    payload_hash.copy_from_slice(hasher.result().as_ref());

                    Some(Credentials::Sgx {
                        requestor: crypto.requestor_pub_key.serialize().to_vec(),
                        enclave: crypto.pub_key.serialize().to_vec(),
                        payload_sha3: payload_hash,
                        enclave_hash: mr_enclave,
                        ias_report: evidence.report,
                        ias_sig: evidence.signature,
                    })
                }
            })
        }
        .into_actor(self)
        .map(move |result, actor, _| {
            actor.ctx.credentials = result?;
            Ok(())
        });

        Box::new(fut)
    }

    #[cfg(not(feature = "sgx"))]
    fn handle(&mut self, _: Initialize, _: &mut Context<Self>) -> Self::Result {
        Box::new(futures::future::ok(()).into_actor(self))
    }
}

impl<R: Runtime> Handler<GetBatchResults> for ExeUnit<R> {
    type Result = <GetBatchResults as Message>::Result;

    fn handle(&mut self, msg: GetBatchResults, _: &mut Context<Self>) -> Self::Result {
        GetBatchResultsResponse(self.state.batch_results(&msg.0))
    }
}

impl<Svc, R> Handler<Register<Svc>> for ExeUnit<R>
where
    R: Runtime,
    Svc: Actor<Context = Context<Svc>> + Handler<Shutdown>,
{
    type Result = <Register<Svc> as Message>::Result;

    fn handle(&mut self, msg: Register<Svc>, _: &mut Context<Self>) -> Self::Result {
        self.services.push(Box::new(ServiceAddr::new(msg.0)));
    }
}

impl<R: Runtime> Handler<SignExeScript> for ExeUnit<R> {
    type Result = <SignExeScript as Message>::Result;

    #[cfg(feature = "sgx")]
    fn handle(&mut self, msg: SignExeScript, _: &mut Context<Self>) -> Self::Result {
        let batch = self.state.batches.get(&msg.batch_id).ok_or_else(|| {
            Error::RuntimeError(format!(
                "signing an unknown ExeScript (batch [{}])",
                msg.batch_id
            ))
        })?;

        let stub = SignatureStub {
            script: batch.exe_script.clone(),
            results: self.state.batch_results(&msg.batch_id),
            digest: "sha3".to_string(),
        };

        let json = serde_json::to_string(&stub)?;
        let sig_vec = self.ctx.crypto.sign(json.as_bytes())?;

        Ok(SignExeScriptResponse {
            output: json,
            sig: hex::encode(&sig_vec),
        })
    }

    #[cfg(not(feature = "sgx"))]
    fn handle(&mut self, _: SignExeScript, _: &mut Context<Self>) -> Self::Result {
        Err(Error::Other(
            "signing not supported: binary built without the 'sgx' feature".into(),
        ))
    }
}

impl<R: Runtime> Handler<Stop> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Stop, _: &mut Context<Self>) -> Self::Result {
        self.state.batch_control.retain(|id, tx| {
            if msg.exclude_batches.contains(id) {
                return true;
            }
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
            false
        });

        let fut = Self::stop_runtime(self.runtime.clone(), ShutdownReason::Interrupted(0))
            .map(|_| Ok(()));
        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        if !self.state.inner.alive() {
            return ActorResponse::r#async(async { Ok(()) }.into_actor(self));
        }

        let address = ctx.address();
        let services = std::mem::replace(&mut self.services, Vec::new());
        let state = self.state.inner.to_pending(State::Terminated);
        let reason = format!("{}: {}", msg.0, self.state.report());

        let fut = async move {
            log::info!("Shutting down ...");
            let _ = address.send(SetState::from(state)).await;
            let _ = address.send(Stop::default()).await;

            for mut service in services {
                service.stop().await;
            }

            let set_state = SetState::default().state_reason(State::Terminated.into(), reason);
            let _ = address.send(set_state).await;

            System::current().stop();

            log::info!("Shutdown process complete");
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self))
    }
}
