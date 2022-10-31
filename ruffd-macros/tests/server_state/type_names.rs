use ruffd_macros::server_state;
use ruffd_types::tokio::sync::RwLock;
use std::sync::Arc;

#[server_state]
pub struct Foo {
    pub foo: u32,
    pub bar: bool,
}

fn main() {
    let new_state = Foo {
        foo: Arc::new(RwLock::new(3)),
        bar: Arc::new(RwLock::new(false)),
    };
    let locks = FooLocks::default();
    let handles_fut = foo_handles_from_locks(locks);
}
