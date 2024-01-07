mod rindexer;

use rindexer::lens_registry_example::events::lens_registry::{
    HandleLinkedEvent, HandleUnlinkedEvent, LensRegistryEventType, NonceUpdatedEvent,
};

use rindexer_core::{
    generator::{build::build, event_callback_registry::EventCallbackRegistry},
    indexer::start::start,
};

#[tokio::main]
async fn main() {
    //generate();

    let mut registry = EventCallbackRegistry::new();

    LensRegistryEventType::NonceUpdated(NonceUpdatedEvent {
        callback: Box::new(|data| {
            println!("NonceUpdated event: {:?}", data);
        }),
    })
    .register(&mut registry);

    LensRegistryEventType::HandleLinked(HandleLinkedEvent {
        callback: Box::new(|data| {
            println!("HandleLinked event: {:?}", data);
        }),
    })
    .register(&mut registry);

    LensRegistryEventType::HandleUnlinked(HandleUnlinkedEvent {
        callback: Box::new(|data| {
            println!("HandleUnlinked event: {:?}", data);
        }),
    })
    .register(&mut registry);

    let result = start(registry).await;

    println!("{:?}", result);
}

fn generate() {
    build(
        "/Users/joshstevens/code/rindexer/rindexer_demo/manifest-example.yaml",
        "/Users/joshstevens/code/rindexer/rindexer_demo/src/rindexer",
    )
    .unwrap();
}
