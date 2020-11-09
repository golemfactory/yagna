
use crate::driver::PaymentDriver;


pub struct PaymentDriverCron {
    driver : Box<dyn PaymentDriver>
}


impl PaymentDriverCron {
    pub fn new(
        driver : Box<dyn PaymentDriver>
    ) -> Addr<Self> {
        let me = Self {
            driver
            // active_accounts,
            // ethereum_client,
            // gnt_contract,
            // db,
            // nonces: Default::default(),
            // next_reservation_id: 0,
            // pending_reservations: Default::default(),
            // pending_confirmations: Default::default(),
            // receipt_queue: Default::default(),
            // reservation: None,
            // required_confirmations: env.required_confirmations,
        };

        me.start()
    }

    fn start_payment_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            act.driver.process_payments().await;
            // for address in act.active_accounts.borrow().list_accounts() {
            //     log::trace!("payment job for: {:?}", address);
            //     match act.active_accounts.borrow().get_node_id(address.as_str()) {
            //         None => continue,
            //         Some(node_id) => {
            //             let account = address.clone();
            //             let client = act.ethereum_client.clone();
            //             let gnt_contract = act.gnt_contract.clone();
            //             let tx_sender = ctx.address();
            //             let db = act.db.clone();
            //             let sign_tx = utils::get_sign_tx(node_id);
            //             Arbiter::spawn(async move {
            //                 process_payments(
            //                     account,
            //                     client,
            //                     gnt_contract,
            //                     tx_sender,
            //                     db,
            //                     &sign_tx,
            //                 )
            //                 .await;
            //             });
            //         }
            //     }
            // }
        });
    }
}


impl Actor for PaymentDriverCron {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // self.start_confirmation_job(ctx);
        // self.start_block_traces(ctx);
        // self.load_txs(ctx);
        self.start_payment_job(ctx);
    }
}
