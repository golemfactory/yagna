use erc20_payment_lib::rpc_pool::Web3RpcPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{rustls, TlsAcceptor};
use web3::{transports::Http, Web3};

// Exercise the client constructed inside ERC20, without injecting a TLS client.
fn erc20_http(url: String) -> Web3<Http> {
    let pool = Web3RpcPool::new_from_urls(1337, vec![url]);
    let endpoints = pool.endpoints.lock();
    let rpc = endpoints
        .iter()
        .next()
        .unwrap()
        .1
        .read()
        .web3
        .clone()
        .unwrap();
    rpc
}

#[tokio::test]
async fn erc20_default_client_rejects_private_ca() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate = certified.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key.into())
        .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://localhost:{}", listener.local_addr().unwrap().port());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let error = acceptor.accept(socket).await.expect_err("untrusted CA must fail TLS");
            assert!(matches!(
                error.get_ref().and_then(|e| e.downcast_ref::<rustls::Error>()),
                Some(rustls::Error::AlertReceived(rustls::AlertDescription::UnknownCA))
            ), "unexpected TLS error: {error}");

            // A client explicitly trusting this certificate must reach the same server.
            // This prevents a broken server or connection failure from passing the test.
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(socket).await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let size = stream.read(&mut buffer).await.unwrap();
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
            }
            let body = r#"{"jsonrpc":"2.0","id":0,"result":"0x539"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = stream.shutdown().await;
        });

        assert!(erc20_http(url.clone()).eth().chain_id().await.is_err());

        let trusted = reqwest::Client::builder()
            .no_proxy()
            .add_root_certificate(reqwest::Certificate::from_der(certificate.as_ref()).unwrap())
            .build()
            .unwrap();
        let rpc = Web3::new(Http::with_client(trusted, url.parse().unwrap()));
        assert_eq!(rpc.eth().chain_id().await.unwrap().as_u64(), 1337);
        server.await.unwrap();
    })
    .await
    .expect("TLS verification must finish within 15 seconds");
}

#[tokio::test]
#[ignore = "live Hoodi RPC smoke test; requires Internet access"]
async fn erc20_default_client_accepts_public_ca() {
    let rpc = erc20_http("https://ethereum-hoodi-rpc.publicnode.com".into());
    let chain_id = tokio::time::timeout(Duration::from_secs(30), rpc.eth().chain_id())
        .await
        .expect("RPC timed out")
        .expect("ERC20 must accept the public RPC certificate using Mozilla roots");
    assert_eq!(chain_id.as_u64(), 560048);
}
