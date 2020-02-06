

/// Actor responsible for direct interaction with ExeUnit trait
/// implementation. Runs in different thread to perform heavy computations.
struct Worker {

}


// =========================================== //
// Actix stuff
// =========================================== //

impl Actor for Worker {
    type Context = Context<Self>;
}



