//! End-to-end: WS client -> relay -> UDP echo server -> relay -> WS client.

use dsor_relay::{serve, Config};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio_tungstenite::tungstenite::Message;

async fn start_echo() -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let port = sock.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            let (n, from) = sock.recv_from(&mut buf).await.unwrap();
            sock.send_to(&buf[..n], from).await.unwrap();
        }
    });
    port
}

async fn start_relay(allow_port: u16) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let cfg = Config {
        listen: listener.local_addr().unwrap(),
        allow_hosts: vec!["127.0.0.1".into()],
        allow_ports: vec![(allow_port, allow_port)],
    };
    tokio::spawn(serve(listener, Arc::new(cfg)));
    port
}

#[tokio::test]
async fn round_trip_datagrams() {
    let echo = start_echo().await;
    let relay = start_relay(echo).await;
    // Percent-encoded colon, as a browser's URLSearchParams would send it.
    let url = format!("ws://127.0.0.1:{relay}/?target=127.0.0.1%3A{echo}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

    let datagrams: Vec<Vec<u8>> = vec![
        vec![0x05, 0x00, 0xff, 0xff, 0x00],
        vec![],
        (0..=255u8).collect(),
        vec![0xAB; 1400],
        vec![0x84, 0x01, 0x02, 0x03],
    ];
    for d in &datagrams {
        ws.send(Message::Binary(d.clone())).await.unwrap();
    }
    for d in &datagrams {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timeout")
            .expect("stream ended")
            .expect("ws error");
        match msg {
            Message::Binary(got) => assert_eq!(&got, d, "datagram boundaries preserved"),
            other => panic!("unexpected {other:?}"),
        }
    }
    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn rejects_targets_outside_allowlist() {
    let echo = start_echo().await;
    let relay = start_relay(echo).await;
    for target in [
        format!("127.0.0.1:{}", echo.wrapping_add(1)), // port not allowed
        format!("10.1.2.3:{echo}"),                    // host not allowed
        "garbage".to_string(),
    ] {
        let url = format!("ws://127.0.0.1:{relay}/?target={target}");
        let err = tokio_tungstenite::connect_async(url).await.unwrap_err();
        match err {
            tokio_tungstenite::tungstenite::Error::Http(resp) => {
                assert_eq!(resp.status(), 403, "target {target}")
            }
            other => panic!("target {target}: expected HTTP 403, got {other:?}"),
        }
    }
    // No target at all -> 400.
    let url = format!("ws://127.0.0.1:{relay}/");
    match tokio_tungstenite::connect_async(url).await.unwrap_err() {
        tokio_tungstenite::tungstenite::Error::Http(resp) => assert_eq!(resp.status(), 400),
        other => panic!("expected HTTP 400, got {other:?}"),
    }
}
