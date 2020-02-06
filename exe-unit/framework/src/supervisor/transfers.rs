use actix::prelude::*;


/// Actor responsible for managing transfers. It should forward
/// transfer requests to implementation of different transfer protocols.
struct Transfers {

}

// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for Transfers {
    type Context = Context<Self>;
}





