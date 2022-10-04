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

impl<R: Runtime> StreamHandler<RuntimeEvent> for ExeUnit<R> {
    fn handle(&mut self, event: RuntimeEvent, ctx: &mut Context<Self>) {
        match event {
            RuntimeEvent::Process(event) => match self.state.batches.get_mut(&event.batch_id) {
                Some(batch) => {
                    let batch_id = event.batch_id.clone();
                    self.state.last_batch = Some(batch_id.clone());

                    if let Err(err) = batch.handle_event(event) {
                        log::error!("Batch {} event error: {}", batch_id, err);
                    }
                }
                _ => log::error!("Batch {} event error: unknown batch", event.batch_id),
            },
            RuntimeEvent::Counter { name, value } => {
                let addr = self.metrics.clone();
                let fut = async move {
                    let _ = addr.send(SetMetric { name, value }).await;
                };
                ctx.spawn(fut.into_actor(self));
            }
            other => {
                log::warn!("Unsupported runtime event: {:?}", other);
            }
        };
    }
}

impl<R: Runtime> Handler<GetState> for ExeUnit<R> {
    type Result = <GetState as Message>::Result;

    fn handle(&mut self, _: GetState, _: &mut Context<Self>) -> Self::Result {
        GetStateResponse(self.state.inner)
    }
}

impl<R: Runtime> Handler<SetState> for ExeUnit<R> {
    //type Result = <SetState as Message>::Result;
    type Result = ActorResponse<Self, ()>;

    fn handle(&mut self, update: SetState, _ctx: &mut Context<Self>) -> Self::Result {
        if self.state.inner == update.state {
            return ActorResponse::reply(());
        }

        log::debug!("Entering state: {:?}", update.state);
        log::debug!("Report: {}", self.state.report());
        self.state.inner = update.state;

        if self.ctx.activity_id.is_none() || self.ctx.report_url.is_none() {
            return ActorResponse::reply(());
        }

        let credentials = match &update.state {
            activity::StatePair(activity::State::Initialized, None) => self.ctx.credentials.clone(),
            _ => None,
        };
        let fut = report(
            self.ctx.report_url.clone().unwrap(),
            SetActivityState::new(
                self.ctx.activity_id.clone().unwrap(),
                activity::ActivityState {
                    state: update.state,
                    reason: update.reason,
                    error_message: None,
                },
                credentials,
            ),
        );

        ActorResponse::r#async(
            async move {
                fut.await;
            }
            .into_actor(self),
        )
    }
}

impl<R: Runtime> Handler<GetStdOut> for ExeUnit<R> {
    type Result = <GetStdOut as Message>::Result;

    fn handle(&mut self, msg: GetStdOut, _: &mut Context<Self>) -> Self::Result {
        self.state
            .batches
            .get(&msg.batch_id)
            .and_then(|b| {
                b.results
                    .get(msg.idx)
                    .and_then(|r| r.stdout.output_string())
            })
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

                    let task_package = task_package.ok_or_else(||
                        Error::Other("Agreement has no `task_package` defined which is mandatory in case of SGX ExeUnit".into())
                    )?;

                    let att_dev = std::path::Path::new("/dev/attestation");
                    if !att_dev.exists() {
                        let msg = format!("'{}' does not exist", att_dev.display());
                        return Err(Error::Attestation(msg));
                    }

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

        Box::pin(fut)
    }

    #[cfg(not(feature = "sgx"))]
    fn handle(&mut self, _: Initialize, _: &mut Context<Self>) -> Self::Result {
        Box::pin(futures::future::ok(()).into_actor(self))
    }
}

impl<R: Runtime> Handler<GetBatchResults> for ExeUnit<R> {
    type Result = <GetBatchResults as Message>::Result;

    fn handle(&mut self, msg: GetBatchResults, _: &mut Context<Self>) -> Self::Result {
        GetBatchResultsResponse(match self.state.batches.get(&msg.batch_id) {
            Some(batch) => batch.results(msg.idx),
            _ => Vec::new(),
        })
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
            script: batch.exec.exe_script.clone(),
            results: batch.results.iter().map(|s| s.repr()).collect(),
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
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: Stop, _: &mut Context<Self>) -> Self::Result {
        self.state.batches.iter_mut().for_each(|(id, batch)| {
            if msg.exclude_batches.contains(id) {
                return;
            }
            if let Some(tx) = batch.control.take() {
                let _ = tx.send(());
            }
        });

        let fut = Self::stop_runtime(self.runtime.clone(), ShutdownReason::Interrupted(0))
            .map(|_| Ok(()));
        ActorResponse::r#async(fut.into_actor(self))
    }
}

impl<R: Runtime> Handler<Shutdown> for ExeUnit<R> {
    type Result = ActorResponse<Self, Result<(), Error>>;

    fn handle(&mut self, msg: Shutdown, ctx: &mut Context<Self>) -> Self::Result {
        if !self.state.inner.alive() {
            return ActorResponse::r#async(async { Ok(()) }.into_actor(self));
        }

        let address = ctx.address();
        let services = std::mem::take(&mut self.services);
        let state = self.state.inner.to_pending(State::Terminated);
        let reason = format!("{}: {}", msg.0, self.state.report());

        let fut = async move {
            log::info!("Shutting down: {}", reason);
            let _ = address.send(SetState::from(state)).await;
            let _ = address.send(Stop::default()).await;

            for mut service in services {
                service.stop().await;
            }

            let set_state = SetState::new(State::Terminated.into(), reason);
            let _ = address.send(set_state).await;

            log::info!("Shutdown process complete");
            Ok(())
        };

        ActorResponse::r#async(fut.into_actor(self).map(|result, _, ctx| {
            ctx.stop();
            result
        }))
    }
}
