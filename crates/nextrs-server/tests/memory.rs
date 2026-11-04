//! Process memory stays bounded while clients stop reading large streamed
//! responses. Its own test binary, so no other test shares the RSS reading.
//!
//!   (cd fixtures/next-app && npm ci && npx next build)
//!   cargo test -p nextrs-server --test memory -- --ignored

mod common;

use std::time::Duration;

use common::{Server, Setup, rss_bytes, verify_stream};

const MIB: usize = 1024 * 1024;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn paused_clients_cannot_grow_process_memory() {
    let server = Server::start(Setup::default()).await;
    // Warm the route so code loading is not counted.
    let mut warm = server.connect().await;
    warm.get("/api/stream?size=65536").await;
    warm.response().await.unwrap();
    drop(warm);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let clients_n = 8;
    let size = 32 * MIB;
    let before = rss_bytes();
    let mut clients = Vec::new();
    for _ in 0..clients_n {
        let mut client = server.connect().await;
        client.get(&format!("/api/stream?size={size}")).await;
        assert_eq!(client.head().await.unwrap().status, 200);
        clients.push(client);
    }
    // Every response has started. Give an unbounded producer time to
    // show itself (without backpressure all 256 MiB lands in host memory
    // within about a second).
    tokio::time::sleep(Duration::from_secs(3)).await;
    let paused = rss_bytes();
    let growth = paused.saturating_sub(before);
    let payload = clients_n * size;
    eprintln!(
        "rss before={} MiB paused={} MiB growth={} MiB for {} MiB of paused responses",
        before / MIB,
        paused / MIB,
        growth / MIB,
        payload / MIB
    );
    // Per response the host holds at most its 64 KiB budget plus one chunk;
    // V8 holds the producer's pending chunk. 32 MiB covers allocator and
    // heap noise; an unbounded path would need about 256 MiB.
    assert!(
        growth < 32 * MIB,
        "RSS grew {} MiB while {} MiB of responses were paused",
        growth / MIB,
        payload / MIB
    );

    assert!(
        server.metrics().backpressure_waits >= clients_n as u64,
        "every producer waited for its client"
    );

    // Every paused response still completes, intact, once read.
    for client in &mut clients {
        let mut offset = 0;
        while let Some(chunk) = client.chunk().await.unwrap() {
            offset = verify_stream(offset, &chunk);
        }
        assert_eq!(offset, size);
    }
    server.wait_for("all completed", |m| m.active() == 0).await;
}
