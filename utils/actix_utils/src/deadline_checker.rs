use actix::prelude::*;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use futures::Future;
use std::collections::HashMap;
use std::pin::Pin;

use crate::{
    actix_signal::{SignalSlot, Subscribe},
    actix_signal_handler,
};

/// Will be sent when deadline elapsed.
#[derive(Message, Clone, Debug)]
#[rtype(result = "()")]
pub struct DeadlineElapsed {
    pub category: String,
    pub deadline: DateTime<Utc>,
    pub id: String,
}

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct TrackDeadline {
    pub category: String,
    pub deadline: DateTime<Utc>,
    pub id: String,
}

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct StopTracking {
    pub id: String,
    pub category: Option<String>,
}

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct StopTrackingCategory {
    pub category: String,
}

#[derive(Clone)]
struct DeadlineDesc {
    deadline: DateTime<Utc>,
    id: String,
}

/// Checks if debit notes are accepted in time.
pub struct DeadlineChecker {
    // Maps Agreement ids to chain of DebitNotes.
    // Vec inside HashMap is ordered by DebitNote deadline.
    deadlines: HashMap<String, Vec<DeadlineDesc>>,

    nearest_deadline: DateTime<Utc>,
    handle: Option<SpawnHandle>,
    callback: SignalSlot<DeadlineElapsed>,
}

actix_signal_handler!(DeadlineChecker, DeadlineElapsed, callback);

impl DeadlineChecker {
    pub fn new() -> DeadlineChecker {
        DeadlineChecker {
            deadlines: HashMap::new(),
            nearest_deadline: Utc::now() + Duration::weeks(50),
            callback: SignalSlot::<DeadlineElapsed>::new(),
            handle: None,
        }
    }

    fn update_deadline(&mut self, ctx: &mut Context<Self>) -> anyhow::Result<()> {
        let top_deadline = self.top_deadline();
        if self.nearest_deadline != top_deadline {
            if let Some(handle) = self.handle.take() {
                ctx.cancel_future(handle);
            }

            let notify_timestamp = top_deadline.clone();
            let wait_duration = (top_deadline - Utc::now())
                .max(Duration::milliseconds(1))
                .to_std()
                .map_err(|e| anyhow!("Failed to convert chrono to std Duration. {}", e))?;

            self.handle = Some(ctx.run_later(wait_duration, move |myself, ctx| {
                myself.on_deadline_elapsed(ctx, notify_timestamp);
            }));

            self.nearest_deadline = top_deadline;
        }
        Ok(())
    }

    fn on_deadline_elapsed(&mut self, ctx: &mut Context<Self>, deadline: DateTime<Utc>) {
        let now = Utc::now();
        assert!(now >= deadline);

        let elapsed = self.drain_elapsed(now);

        for event in elapsed.into_iter() {
            self.callback.send_signal(event).ok();
        }

        self.handle.take();
        self.update_deadline(ctx).ok();
    }

    fn drain_elapsed(&mut self, timestamp: DateTime<Utc>) -> Vec<DeadlineElapsed> {
        let mut elapsed = self
            .deadlines
            .iter_mut()
            .map(|(agreement_id, deadlines)| {
                let idx =
                    match deadlines.binary_search_by(|element| element.deadline.cmp(&timestamp)) {
                        Ok(idx) => idx + 1,
                        Err(idx) => idx,
                    };
                deadlines
                    .drain(0..idx)
                    .map(|desc| DeadlineElapsed {
                        category: agreement_id.to_string(),
                        deadline: desc.deadline,
                        id: desc.id,
                    })
                    .collect::<Vec<DeadlineElapsed>>()
                    .into_iter()
            })
            .flatten()
            .collect::<Vec<DeadlineElapsed>>();

        elapsed.sort_by(|dead1, dead2| dead1.deadline.cmp(&dead2.deadline));

        // Remove Agreements with empty lists. Otherwise no longer needed Agreements
        // would remain in HashMap for always.
        self.deadlines = self
            .deadlines
            .drain()
            .filter(|(_, value)| !value.is_empty())
            .collect();
        elapsed
    }

    fn top_deadline(&self) -> DateTime<Utc> {
        let nearest = self
            .deadlines
            .iter()
            .filter_map(|element| {
                let dead_vec = element.1;
                if dead_vec.len() > 0 {
                    Some(dead_vec[0].deadline.clone())
                } else {
                    None
                }
            })
            .min();

        match nearest {
            Some(deadline) => deadline,
            None => Utc::now() + Duration::weeks(50),
        }
    }
}

impl Handler<TrackDeadline> for DeadlineChecker {
    type Result = ();

    fn handle(&mut self, msg: TrackDeadline, ctx: &mut Context<Self>) -> Self::Result {
        if let None = self.deadlines.get(&msg.category) {
            self.deadlines.insert(msg.category.to_string(), vec![]);
        }

        let deadlines = self.deadlines.get_mut(&msg.category).unwrap();
        let idx = match deadlines.binary_search_by(|element| element.deadline.cmp(&msg.deadline)) {
            // Element with this deadline existed. We add new element behind it (order shouldn't matter since timestamps
            // are the same, but it's better to keep order of calls to `track_deadline` function).
            Ok(idx) => idx + 1,
            // Element doesn't exists. Index where it can be inserted is returned here.
            Err(idx) => idx,
        };

        deadlines.insert(
            idx,
            DeadlineDesc {
                deadline: msg.deadline,
                id: msg.id,
            },
        );

        self.update_deadline(ctx).unwrap();
    }
}

impl Handler<StopTracking> for DeadlineChecker {
    type Result = ();

    fn handle(&mut self, msg: StopTracking, ctx: &mut Context<Self>) -> Self::Result {
        let mut any = false;
        // We could store inverse mapping from entities to agreements, but there will never
        // be so many Agreements at the same time, to make it worth.
        for deadlines in self.deadlines.values_mut() {
            if let Some(idx) = deadlines.iter().position(|element| &element.id == &msg.id) {
                // Or we could remove all earlier entries??
                deadlines.remove(idx);
                any = true;
            }
        }

        if any {
            self.update_deadline(ctx).unwrap();
        }
    }
}

impl Handler<StopTrackingCategory> for DeadlineChecker {
    type Result = ();

    fn handle(&mut self, msg: StopTrackingCategory, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(_) = self.deadlines.remove(&msg.category) {
            self.update_deadline(ctx).unwrap()
        }
    }
}

impl Actor for DeadlineChecker {
    type Context = Context<Self>;
}

struct DeadlineFun {
    callback: Box<dyn FnMut(DeadlineElapsed) -> Pin<Box<dyn Future<Output = ()>>> + 'static>,
}

impl Actor for DeadlineFun {
    type Context = Context<Self>;
}

impl Handler<DeadlineElapsed> for DeadlineFun {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: DeadlineElapsed, _ctx: &mut Context<Self>) -> Self::Result {
        (self.callback)(msg)
    }
}

pub async fn bind_deadline_reaction(
    checker: Addr<DeadlineChecker>,
    callback: impl FnMut(DeadlineElapsed) -> Pin<Box<dyn Future<Output = ()>>> + 'static,
) -> anyhow::Result<()> {
    let callback_actor = DeadlineFun {
        callback: Box::new(callback),
    }
    .start();
    Ok(checker
        .send(Subscribe::<DeadlineElapsed>(callback_actor.recipient()))
        .await?)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::actix_signal::Subscribe;

    struct DeadlineReceiver {
        elapsed: Vec<DeadlineElapsed>,
    }

    impl Actor for DeadlineReceiver {
        type Context = Context<Self>;
    }

    impl DeadlineReceiver {
        pub fn new() -> Addr<DeadlineReceiver> {
            DeadlineReceiver { elapsed: vec![] }.start()
        }
    }

    #[derive(Message, Clone)]
    #[rtype(result = "Vec<DeadlineElapsed>")]
    pub struct Collect;

    impl Handler<DeadlineElapsed> for DeadlineReceiver {
        type Result = ();

        fn handle(&mut self, msg: DeadlineElapsed, _ctx: &mut Context<Self>) -> Self::Result {
            self.elapsed.push(msg);
        }
    }

    impl Handler<Collect> for DeadlineReceiver {
        type Result = MessageResult<Collect>;

        fn handle(&mut self, _msg: Collect, _ctx: &mut Context<Self>) -> Self::Result {
            MessageResult(self.elapsed.drain(..).collect())
        }
    }

    async fn init_checker(receiver: Addr<DeadlineReceiver>) -> Addr<DeadlineChecker> {
        let checker = DeadlineChecker::new().start();
        checker
            .send(Subscribe::<DeadlineElapsed>(receiver.recipient()))
            .await
            .unwrap();
        checker
    }

    #[cfg_attr(not(feature = "time-dependent-tests"), ignore)]
    #[actix_rt::test]
    async fn test_deadline_checker_single_agreement() {
        let receiver = DeadlineReceiver::new();
        let checker = init_checker(receiver.clone()).await;

        let now = Utc::now();
        for i in 1..6 {
            checker
                .send(TrackDeadline {
                    category: "agrrrrr-1".to_string(),
                    deadline: now + Duration::milliseconds(500 * i),
                    id: i.to_string(),
                })
                .await
                .unwrap();
        }

        let interval = (now + Duration::milliseconds(1200)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 2);
        assert_eq!(deadlined[0].id, 1.to_string());
        assert_eq!(deadlined[1].id, 2.to_string());

        let interval = (now + Duration::milliseconds(2600)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 3);
        assert_eq!(deadlined[0].id, 3.to_string());
        assert_eq!(deadlined[1].id, 4.to_string());
        assert_eq!(deadlined[2].id, 5.to_string());

        // Add another entry to check if there wasn't any incorrect state,
        // after all deadlines elapsed.
        checker
            .send(TrackDeadline {
                category: "agrrrrr-1".to_string(),
                deadline: now + Duration::milliseconds(3000),
                id: 6.to_string(),
            })
            .await
            .unwrap();

        let interval = (now + Duration::milliseconds(3200)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 1);
        assert_eq!(deadlined[0].id, 6.to_string());
    }

    #[cfg_attr(not(feature = "time-dependent-tests"), ignore)]
    #[actix_rt::test]
    async fn test_deadline_checker_near_deadlines() {
        let receiver = DeadlineReceiver::new();
        let checker = init_checker(receiver.clone()).await;

        let now = Utc::now();
        for i in 1..6 {
            checker
                .send(TrackDeadline {
                    category: "agrrrrr-1".to_string(),
                    deadline: now + Duration::milliseconds(200) + Duration::milliseconds(1 * i),
                    id: i.to_string(),
                })
                .await
                .unwrap();
        }

        // Add another deadline at the same timestamp as already existing one.
        checker
            .send(TrackDeadline {
                category: "agrrrrr-1".to_string(),
                deadline: now + Duration::milliseconds(200) + Duration::milliseconds(3),
                id: 7.to_string(),
            })
            .await
            .unwrap();

        let interval = (now + Duration::milliseconds(300)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 6);
        assert_eq!(deadlined[0].id, 1.to_string());
        assert_eq!(deadlined[1].id, 2.to_string());
        assert_eq!(deadlined[2].id, 3.to_string());
        assert_eq!(deadlined[3].id, 7.to_string());
        assert_eq!(deadlined[4].id, 4.to_string());
        assert_eq!(deadlined[5].id, 5.to_string());
    }

    #[cfg_attr(not(feature = "time-dependent-tests"), ignore)]
    #[actix_rt::test]
    async fn test_deadline_checker_insert_deadlines_between() {
        let receiver = DeadlineReceiver::new();
        let checker = init_checker(receiver.clone()).await;

        let now = Utc::now();
        for i in 1..6 {
            checker
                .send(TrackDeadline {
                    category: "agrrrrr-1".to_string(),
                    deadline: now + Duration::milliseconds(1000) + Duration::milliseconds(500 * i),
                    id: i.to_string(),
                })
                .await
                .unwrap();
        }

        let interval = (now + Duration::milliseconds(100)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        // Insert deadline before all other deadlines.
        checker
            .send(TrackDeadline {
                category: "agrrrrr-1".to_string(),
                deadline: now + Duration::milliseconds(500),
                id: 6.to_string(),
            })
            .await
            .unwrap();

        let interval = (now + Duration::milliseconds(900)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 1);
        assert_eq!(deadlined[0].id, 6.to_string());

        // Insert deadline between all other deadlines.
        checker
            .send(TrackDeadline {
                category: "agrrrrr-1".to_string(),
                deadline: now + Duration::milliseconds(2300),
                id: 7.to_string(),
            })
            .await
            .unwrap();

        let interval = (now + Duration::milliseconds(2700)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 4);
        assert_eq!(deadlined[0].id, 1.to_string());
        assert_eq!(deadlined[1].id, 2.to_string());
        assert_eq!(deadlined[2].id, 7.to_string());
        assert_eq!(deadlined[3].id, 3.to_string());
    }

    #[cfg_attr(not(feature = "time-dependent-tests"), ignore)]
    #[actix_rt::test]
    async fn test_deadline_checker_multi_agreements() {
        let receiver = DeadlineReceiver::new();
        let checker = init_checker(receiver.clone()).await;

        let now = Utc::now();
        for i in 1..6 {
            checker
                .send(TrackDeadline {
                    category: format!("agrrrrr-{}", i),
                    deadline: now
                        + Duration::milliseconds(100)
                        + Duration::milliseconds(6 * 3 - 3 * i),
                    id: i.to_string(),
                })
                .await
                .unwrap();
        }

        let interval = (now + Duration::milliseconds(500)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();
        assert_eq!(deadlined.len(), 5);
        assert_eq!(deadlined[0].id, 5.to_string());
        assert_eq!(deadlined[1].id, 4.to_string());
        assert_eq!(deadlined[2].id, 3.to_string());
        assert_eq!(deadlined[3].id, 2.to_string());
        assert_eq!(deadlined[4].id, 1.to_string());
    }

    #[cfg_attr(not(feature = "time-dependent-tests"), ignore)]
    #[actix_rt::test]
    async fn test_deadline_checker_stop_tracking() {
        let receiver = DeadlineReceiver::new();
        let checker = init_checker(receiver.clone()).await;

        let now = Utc::now();
        for i in 1..8 {
            checker
                .send(TrackDeadline {
                    category: "agrrrrr-1".to_string(),
                    deadline: now + Duration::milliseconds(500 * i),
                    id: i.to_string(),
                })
                .await
                .unwrap();
        }

        let interval = (now + Duration::milliseconds(100)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        checker
            .send(StopTracking {
                id: 2.to_string(),
                category: None,
            })
            .await
            .unwrap();
        checker
            .send(StopTracking {
                id: 5.to_string(),
                category: None,
            })
            .await
            .unwrap();

        let interval = (now + Duration::milliseconds(3900)) - Utc::now();
        tokio::time::sleep(interval.to_std().unwrap()).await;

        let deadlined = receiver.send(Collect {}).await.unwrap();

        assert_eq!(deadlined.len(), 5);
        assert_eq!(deadlined[0].id, 1.to_string());
        assert_eq!(deadlined[1].id, 3.to_string());
        assert_eq!(deadlined[2].id, 4.to_string());
        assert_eq!(deadlined[3].id, 6.to_string());
        assert_eq!(deadlined[4].id, 7.to_string());
    }
}
