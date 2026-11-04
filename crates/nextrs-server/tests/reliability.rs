//! Executor reliability through real sockets and the real accept loop:
//! backpressure from slow clients, disconnects, write timeouts, request
//! body limits, overload, the connection cap and graceful shutdown.
//!
//!   (cd fixtures/next-app && npm ci && npx next build)
//!   cargo test -p nextrs-server --test reliability -- --ignored

mod common;

use std::time::{Duration, Instant};

use common::{Server, Setup, chunked_body, payload, sha256_hex, verify_stream};
use nextrs_runtime::Limits;
use nextrs_server::ServeOptions;

const MIB: usize = 1024 * 1024;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_client_that_pauses_then_resumes_gets_the_whole_ordered_body() {
    let server = Server::start(Setup::default()).await;
    let size = 8 * MIB;
    let mut client = server.connect().await;
    client.get(&format!("/api/stream?size={size}")).await;
    let head = client.head().await.unwrap();
    assert_eq!(head.status, 200);
    assert!(head.is_chunked());

    // Stop reading: the producer must stall on the response budget.
    server
        .wait_for("backpressure", |m| m.backpressure_waits > 0)
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let waits = server.metrics().backpressure_waits;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        server.metrics().backpressure_waits,
        waits,
        "a paused producer stays parked; it does not spin"
    );
    assert_eq!(server.metrics().active(), 1);

    let mut offset = 0;
    while let Some(chunk) = client.chunk().await.unwrap() {
        offset = verify_stream(offset, &chunk);
    }
    assert_eq!(offset, size);
    server
        .wait_for("slot released", |m| {
            m.active() == 0 && m.requests_completed == 1
        })
        .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_disconnect_mid_stream_cancels_the_request() {
    let server = Server::start(Setup::default()).await;
    let mut client = server.connect().await;
    client.get(&format!("/api/stream?size={}", 64 * MIB)).await;
    assert_eq!(client.head().await.unwrap().status, 200);
    let first = client.chunk().await.unwrap().unwrap();
    verify_stream(0, &first);
    server
        .wait_for("backpressure", |m| m.backpressure_waits > 0)
        .await;
    drop(client);
    server
        .wait_for("cancellation", |m| {
            m.responses_cancelled == 1 && m.active() == 0
        })
        .await;

    // The isolate is unaffected.
    let mut client = server.connect().await;
    client.get("/api/health").await;
    assert_eq!(client.response().await.unwrap().0.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_client_that_stops_reading_is_closed_after_the_write_timeout() {
    let server = Server::start(Setup {
        serve: ServeOptions {
            write_timeout: Some(Duration::from_secs(1)),
            ..ServeOptions::default()
        },
        ..Setup::default()
    })
    .await;
    let mut client = server.connect().await;
    client.get(&format!("/api/stream?size={}", 256 * MIB)).await;
    assert_eq!(client.head().await.unwrap().status, 200);
    let started = Instant::now();
    // Never read again. Kernel socket buffers fill first, then the write
    // stalls and the timeout closes the connection.
    server
        .wait_for("write timeout", |m| {
            m.responses_cancelled == 1 && m.active() == 0
        })
        .await;
    assert!(started.elapsed() >= Duration::from_secs(1));
    assert!(client.closed_within(Duration::from_secs(5)).await);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn request_body_limits_hold_for_declared_and_chunked_lengths() {
    let server = Server::start(Setup {
        max_request_body: MIB as u64,
        ..Setup::default()
    })
    .await;

    // A declared length over the limit never reaches JavaScript.
    let mut client = server.connect().await;
    client
        .send(
            format!(
                "POST /api/echo HTTP/1.1\r\nhost: x\r\ncontent-length: {}\r\n\r\n",
                2 * MIB
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let (head, _) = client.response().await.unwrap();
    assert_eq!(head.status, 413);
    assert_eq!(
        head.header("x-nextrs-error"),
        Some("request-body-too-large")
    );
    assert_eq!(head.header("connection"), Some("close"));
    assert_eq!(server.metrics().requests_admitted, 0);

    // A chunked body with no declared length is counted as it streams.
    let mut client = server.connect().await;
    let body = chunked_body(&payload(2 * MIB, 1), 64 * 1024);
    client
        .send(b"POST /api/echo HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\n")
        .await
        .unwrap();
    // The server may stop reading (and close) before the whole body is
    // sent; that is the point.
    let _ = client.send(&body).await;
    let (head, _) = client.response().await.unwrap();
    assert_eq!(head.status, 413);
    server
        .wait_for("over-limit request released", |m| m.active() == 0)
        .await;

    // Within the limit, chunked: the handler hashes what it read.
    let data = payload(MIB - 1, 2);
    let mut client = server.connect().await;
    client
        .send(b"POST /api/echo HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\n")
        .await
        .unwrap();
    client.send(&chunked_body(&data, 16 * 1024)).await.unwrap();
    let (head, body) = client.response().await.unwrap();
    assert_eq!(head.status, 200);
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["bytes"], data.len());
    assert_eq!(json["sha256"], sha256_hex(&data));
    assert_eq!(json["chunked"], true);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_malformed_chunked_body_is_rejected_without_hanging() {
    let server = Server::start(Setup::default()).await;
    let mut client = server.connect().await;
    client
        .send(b"POST /api/echo HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\nzz\r\nnot hex\r\n")
        .await
        .unwrap();
    // hyper fails the body stream; the handler sees an aborted upload and
    // the connection is closed rather than left waiting.
    assert!(client.closed_within(Duration::from_secs(10)).await);
    server
        .wait_for("malformed upload released", |m| m.active() == 0)
        .await;
    let mut client = server.connect().await;
    client.get("/api/health").await;
    assert_eq!(client.response().await.unwrap().0.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn an_aborted_upload_releases_the_request() {
    let server = Server::start(Setup::default()).await;
    let mut client = server.connect().await;
    client
        .send(b"POST /api/echo HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\n")
        .await
        .unwrap();
    let partial = chunked_body(&payload(256 * 1024, 3), 16 * 1024);
    // Everything but the terminating chunk, then the client vanishes.
    client.send(&partial[..partial.len() - 5]).await.unwrap();
    server
        .wait_for("upload delivered", |m| m.requests_admitted == 1)
        .await;
    drop(client);
    server
        .wait_for("aborted upload released", |m| m.active() == 0)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_streamed_upload_can_be_streamed_back() {
    let server = Server::start(Setup::default()).await;
    let data = payload(3 * MIB, 4);
    let mut client = server.connect().await;
    client
        .send(
            b"POST /api/echo?mode=stream HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\n\r\n",
        )
        .await
        .unwrap();
    // Upload and download at the same time: the echo only progresses as
    // both sides do.
    let (mut reader, mut writer) = client.stream.into_split();
    let upload = {
        let body = chunked_body(&data, 32 * 1024);
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            writer.write_all(&body).await.unwrap();
            writer
        })
    };
    let mut raw = Vec::new();
    {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if raw.ends_with(b"\r\n0\r\n\r\n") {
                break;
            }
        }
    }
    let _writer = upload.await.unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert!(String::from_utf8_lossy(&raw[..split]).starts_with("HTTP/1.1 200"));
    let mut body = Vec::new();
    let mut rest = &raw[split + 4..];
    loop {
        let line_end = rest.windows(2).position(|w| w == b"\r\n").unwrap();
        let size =
            usize::from_str_radix(std::str::from_utf8(&rest[..line_end]).unwrap(), 16).unwrap();
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        body.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    assert_eq!(body.len(), data.len());
    assert!(body == data, "echoed body differs");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_saturated_executor_refuses_with_503_and_recovers() {
    let server = Server::start(Setup {
        limits: Limits {
            max_active_per_isolate: 1,
            max_queued: 0,
            ..Limits::default()
        },
        ..Setup::default()
    })
    .await;
    // A paused streaming response holds the only slot.
    let mut holder = server.connect().await;
    holder.get(&format!("/api/stream?size={}", 64 * MIB)).await;
    assert_eq!(holder.head().await.unwrap().status, 200);

    let mut client = server.connect().await;
    client.get("/api/health").await;
    let (head, _) = client.response().await.unwrap();
    assert_eq!(head.status, 503);
    assert_eq!(head.header("retry-after"), Some("1"));
    assert_eq!(head.header("x-nextrs-error"), Some("executor-overloaded"));
    assert_eq!(server.metrics().rejected_overloaded, 1);

    // Static output never needs the executor.
    client.get("/").await;
    assert_eq!(client.response().await.unwrap().0.status, 200);

    drop(holder);
    server.wait_for("slot released", |m| m.active() == 0).await;
    client.get("/api/health").await;
    assert_eq!(client.response().await.unwrap().0.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn queued_requests_time_out_with_503() {
    let server = Server::start(Setup {
        limits: Limits {
            max_active_per_isolate: 1,
            max_queued: 4,
            queue_timeout: Duration::from_millis(300),
            ..Limits::default()
        },
        ..Setup::default()
    })
    .await;
    let mut holder = server.connect().await;
    holder.get(&format!("/api/stream?size={}", 64 * MIB)).await;
    assert_eq!(holder.head().await.unwrap().status, 200);

    let started = Instant::now();
    let mut client = server.connect().await;
    client.get("/api/health").await;
    let (head, _) = client.response().await.unwrap();
    assert_eq!(head.status, 503);
    assert_eq!(
        head.header("x-nextrs-error"),
        Some("executor-queue-timeout")
    );
    assert!(started.elapsed() >= Duration::from_millis(300));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn connections_beyond_the_cap_wait_for_a_free_one() {
    let server = Server::start(Setup {
        serve: ServeOptions {
            max_connections: 1,
            ..ServeOptions::default()
        },
        ..Setup::default()
    })
    .await;
    let mut first = server.connect().await;
    first.get("/api/health").await;
    assert_eq!(first.response().await.unwrap().0.status, 200);

    // The idle keep-alive connection holds the only permit.
    let mut second = server.connect().await;
    second.get("/api/health").await;
    let waited = tokio::time::timeout(Duration::from_millis(500), second.response()).await;
    assert!(waited.is_err(), "the second connection was served early");

    drop(first);
    let (head, _) = tokio::time::timeout(Duration::from_secs(5), second.response())
        .await
        .expect("served once the first connection closed")
        .unwrap();
    assert_eq!(head.status, 200);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn graceful_shutdown_finishes_streams_and_refuses_new_connections() {
    let mut server = Server::start(Setup::default()).await;
    let size = 4 * MIB;
    let mut client = server.connect().await;
    client.get(&format!("/api/stream?size={size}")).await;
    assert_eq!(client.head().await.unwrap().status, 200);
    server
        .wait_for("backpressure", |m| m.backpressure_waits > 0)
        .await;

    server.begin_shutdown();
    let addr = server.addr;
    let refused = async {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if tokio::net::TcpStream::connect(addr).await.is_err() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    };
    assert!(refused.await, "new connections are refused while draining");

    let mut offset = 0;
    while let Some(chunk) = client.chunk().await.unwrap() {
        offset = verify_stream(offset, &chunk);
    }
    assert_eq!(offset, size, "the in-flight stream completed");
    drop(client);
    server.stopped().await;
    assert_eq!(server.metrics().ready_executors(), 0);
}

fn restarts(server: &Server) -> u64 {
    server.metrics().executors.iter().map(|e| e.restarts).sum()
}

fn failures(server: &Server) -> u64 {
    server
        .metrics()
        .executors
        .iter()
        .flat_map(|e| e.failures.iter().map(|(_, n)| *n))
        .sum()
}

/// Next's route module installs process error handlers that log and
/// continue, as `next start` does; NextRS must not restart the isolate (and
/// fail its other requests) for an application error after the response.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn uncaught_errors_in_next_routes_are_logged_not_fatal() {
    let server = Server::start(Setup::default()).await;
    // In flight on the same (only) isolate the whole time.
    let size = 2 * MIB;
    let mut bystander = server.connect().await;
    bystander.get(&format!("/api/stream?size={size}")).await;
    assert_eq!(bystander.head().await.unwrap().status, 200);

    let mut client = server.connect().await;
    for kind in ["uncaught", "rejection", "uncaught", "rejection"] {
        client.get(&format!("/api/faults?kind={kind}")).await;
        assert_eq!(client.response().await.unwrap().0.status, 200);
    }
    // Let the timers fire and the rejections surface.
    tokio::time::sleep(Duration::from_millis(300)).await;
    client.get("/api/health").await;
    assert_eq!(client.response().await.unwrap().0.status, 200);
    assert_eq!(restarts(&server), 0, "{:#?}", server.metrics().executors);
    assert_eq!(failures(&server), 0);

    let mut offset = 0;
    while let Some(chunk) = bystander.chunk().await.unwrap() {
        offset = verify_stream(offset, &chunk);
    }
    assert_eq!(offset, size, "the bystander request was not disturbed");
}

/// A route that never yields is stopped by terminating its whole isolate:
/// the request gets 502, requests sharing the isolate are cut off, and a
/// fresh isolate (Next modules reloaded) serves the next request.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_cpu_stall_in_a_next_route_replaces_the_isolate() {
    let server = Server::start(Setup {
        limits: Limits {
            stall_timeout: Some(Duration::from_millis(500)),
            restart_backoff_initial: Duration::from_millis(10),
            ..Limits::default()
        },
        ..Setup::default()
    })
    .await;
    let mut bystander = server.connect().await;
    bystander
        .get(&format!("/api/stream?size={}", 64 * MIB))
        .await;
    assert_eq!(bystander.head().await.unwrap().status, 200);

    let started = Instant::now();
    let mut client = server.connect().await;
    client.get("/api/spin?ms=20000").await;
    let (head, _) = client.response().await.unwrap();
    assert_eq!(head.status, 502);
    assert_eq!(head.header("x-nextrs-error"), Some("executor-failed"));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "terminated well before the loop ended"
    );

    // The bystander's response is aborted, not ended cleanly.
    let mut cut = false;
    loop {
        match bystander.chunk().await {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                cut = true;
                break;
            }
        }
    }
    assert!(
        cut,
        "a response from the terminated isolate looked complete"
    );

    server
        .wait_for("isolate replaced", |m| {
            m.ready_executors() == 1 && m.executors[0].restarts == 1
        })
        .await;
    let snapshot = server.metrics();
    assert!(
        snapshot.executors[0]
            .last_failure
            .as_deref()
            .is_some_and(|f| f.starts_with("cpu_stall")),
        "{:?}",
        snapshot.executors[0]
    );
    let mut client = server.connect().await;
    client.get("/request-info").await;
    let (head, body) = client.response().await.unwrap();
    assert_eq!(head.status, 200);
    assert!(String::from_utf8_lossy(&body).contains("Request info"));
}
