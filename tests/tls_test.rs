//! TLS integration test for the cluster server.
//!
//! Spins up a single-node cluster served over TLS with an API token, then drives a
//! real HTTPS handshake with `curl` to prove: (1) the server speaks TLS, (2) the
//! bearer-token gate still applies over TLS, and (3) plaintext HTTP is refused.
//!
//! The test is skipped (passes) if `curl` is not installed on the host.

use blockdb::cluster::{build_cluster_config, ClusterNode, ClusterSecurity, TlsConfig};
use blockdb::storage::BlockDBConfig;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tempfile::TempDir;
use tokio::process::Command;

static NEXT_PORT: AtomicU16 = AtomicU16::new(19700);

// A self-signed cert/key pair for CN=localhost (test-only, not a real secret).
const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIDCDCCAfCgAwIBAgITPw2Evzalx0vgk34Cayys6sVivDANBgkqhkiG9w0BAQsF
ADAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwHhcNMjYxMTEyMDE1NDI0WhcNMjcxMTEy
MDE1NDI0WjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEBAQUA
A4IBDwAwggEKAoIBAQC8PRw9+EXTwREKCR+RphCJFnZ9nLbcQssCgJcom8K8Fr9z
T77wam5YBcSzr988B1G45PNHCD2ntb+wpwv2FVFzjwLFn520v/B9iOVnWrt8wEgA
afLW/gdl241ah4nCAlYg7gcNGdkxe8v13078adD+uXJy369R7s2eat/DcGJKMa4n
3gFSSTMOR0z8gdAiHkKiTvMj/LS8MiKvpOiIB3PYpouQGZ0vKkYLfOpZgBjyhA2N
L0ZjrJx4blTqEX9LQsiMSy1aHhxaoFd9FQNIMmoPsqba17heUNQHpcIKsVM6dnZx
QGdGDoMZwgMwALDb0hsPNLX7gOD8zu+thXQA9rShAgMBAAGjUzBRMB0GA1UdDgQW
BBQm3kQNZ+FwzQjSZ1UYfQ+vy6hspzAfBgNVHSMEGDAWgBQm3kQNZ+FwzQjSZ1UY
fQ+vy6hspzAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQA//iBt
TL9B6AQsb8mSmSJ9pgdhfo2PhxDLAOrtNAQdbLpkkDKAiBHVtUbKeSMp+pgr6ef8
e8eUu8UX0Nw/Jm5Ape4lPTD3+RW0p1fSQh3V3dni/zY3Tzxa7QscKVhW24joS0sR
EI6J4VNpURL5Mu2mZFii5jm+Bg3ww2pb9MRyjmeLqZEsetSWQqiIq+C/lGqd0Hm2
PFJl8iiKXJwCWbhmbakDS8WPFmF2lq/ko2hgy/0McuQotP2Nq6DsobmraJVuAMzt
lC4SQYVLVTqDer9eyfgeo5GxduYKC+tzv+HbCdTLqGIQYs1NlGCOANkWVkLJ16zw
tkjN+VCuqA3tq9Xr
-----END CERTIFICATE-----
";

const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC8PRw9+EXTwREK
CR+RphCJFnZ9nLbcQssCgJcom8K8Fr9zT77wam5YBcSzr988B1G45PNHCD2ntb+w
pwv2FVFzjwLFn520v/B9iOVnWrt8wEgAafLW/gdl241ah4nCAlYg7gcNGdkxe8v1
3078adD+uXJy369R7s2eat/DcGJKMa4n3gFSSTMOR0z8gdAiHkKiTvMj/LS8MiKv
pOiIB3PYpouQGZ0vKkYLfOpZgBjyhA2NL0ZjrJx4blTqEX9LQsiMSy1aHhxaoFd9
FQNIMmoPsqba17heUNQHpcIKsVM6dnZxQGdGDoMZwgMwALDb0hsPNLX7gOD8zu+t
hXQA9rShAgMBAAECggEABxh50aj9Xz4OP/j+d6VxRuzR12pQ4K/XEM3hsZlgsQMq
gSFznekx8tKrPeeJqRM3J83f91pLXxPUZfqy0lPhyuyMVJnvsYbxFpcfQdLpvlbn
kxxrSvwuyzTuq+4jNmz5uJDZ//4GPrDfJb8ex+UCCJgOkfkpYq1ELI4nlFzOiuPD
+B8r7T3Ae0c9Zso0xiVSjD9a8Sltk3+guKZeA43XV5EBTcDDo4nWklcCSLuNZu/E
IuPBVi4l6640WW9J7bI8Qkxwfv5+JWkpVVU3z8cARLzbtJQosDHFSxjHg3uUGqKZ
jj7LBTjNFx2aIp8+u4RGdDA15K/EQMZpe0Z2UrqBAQKBgQDhlww1BLa5kgcZbcQp
eUOcyERFNljXTbVFCb18Vc+4c4EP9jPwV/k9iYAjpyAhtSuu7XvPJu6oZ841cbI5
zss67ZUcn2o2f/NJQ728waSjh2fpXwakFYhX+6FVvOZKisxodyXlm/rKko2YFISz
gHhNbskaq2Vrq4DiqnuOk3NzgwKBgQDVnRhXniTy6Ys9YNqg/F4OzubIvBwJhaiI
Ow4xd/PzNTF/C8FixzRL42YUUcUiFywdBa5ipJaQjzQEJmuGIVdZwEv+TQu8/mU6
JuvMo9uyvvkeftCzAREzaw5urk07cPWO4luJNPSkTwzT3yefD27SYwu4+0ryRuor
WERsw/nqCwKBgADWIYp0yQSpIPOcL5UDyRdkurQS/oNV2201GwnB+7IwY3Mes5SZ
jugGgX4gWYMme9H7OXFMMKhZMi+Ki2GhfalrRhlEEg2/rbRA1wxDH4wnrV/1JT4o
gkfpO/DocWYnW5irkG1ohaaIopPEx7UurcODvKUnq4mZqjGs9iZSL6RnAoGBAJ1+
9jOjphpzlvXqM0f+ylqgsgfOqd1bbqcC47QVfQgUcCn7wNnThftt7hWNNIuuChhb
1/di4qAy9WcZ7K04Z+8EmQMmS0FNT9VQXQrExXdE1uhlF/rjDouao0l3ufnZjzOK
MXM4WCfNIOgV4GCmQE9eN4ZrwRZgssbCjbCjzd+BAoGALhSNjhzo9vQzEL0NQ9lH
7uSZCIf938km0mdPj//erH1jdy9facpk8N+T160R8qGlvqdC9hbha1iFSSm9RJ8W
taLKObrC1sqghO4O5T+qAPDTMUQzpvNV9V5Qj83YuT79u02RvTQiWCfN2nGvLWAe
VrN/s5qrBXF1HTT8YQ7vT+s=
-----END PRIVATE KEY-----
";

async fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run curl and return (exit_ok, stdout_trimmed).
async fn curl(args: &[&str]) -> (bool, String) {
    let out = Command::new("curl").args(args).output().await.unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cluster_serves_tls_with_auth() {
    if !curl_available().await {
        eprintln!("curl not available; skipping TLS test");
        return;
    }

    let certs_dir = TempDir::new().unwrap();
    let cert_path = certs_dir.path().join("cert.pem");
    let key_path = certs_dir.path().join("key.pem");
    std::fs::write(&cert_path, TEST_CERT).unwrap();
    std::fs::write(&key_path, TEST_KEY).unwrap();

    let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    let data_dir = TempDir::new().unwrap();
    let storage_config = BlockDBConfig {
        data_dir: data_dir.path().to_string_lossy().to_string(),
        ..Default::default()
    };
    let cluster_config = build_cluster_config(
        "node0",
        "127.0.0.1",
        port,
        &[("node0".to_string(), "127.0.0.1".to_string(), port)],
        Duration::from_millis(50),
        Duration::from_millis(150),
    );
    let security = ClusterSecurity {
        cluster_secret: None,
        api_token: Some("tok".to_string()),
        tls: Some(TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
        }),
    };

    let node = ClusterNode::new_secure(cluster_config, storage_config, security).unwrap();
    let n = node.clone();
    tokio::spawn(async move {
        let _ = n.start(std::future::pending::<()>()).await;
    });

    // Wait for the TLS listener + leader.
    let mut elected = false;
    for _ in 0..100 {
        if node.is_leader().await {
            elected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(elected, "single node did not become leader");

    let url = format!("https://127.0.0.1:{}/status", port);
    // -k: accept the self-signed cert. A successful HTTPS handshake proves TLS.
    let (ok, code) = curl(&["-sk", "-o", "/dev/null", "-w", "%{http_code}", &url]).await;
    assert!(ok, "curl HTTPS request failed (TLS handshake?)");
    assert_eq!(code, "200", "/status over TLS should be 200");

    // Write without a token → 401, even over TLS.
    let kv_url = format!("https://127.0.0.1:{}/kv", port);
    let (_ok, code) = curl(&[
        "-sk", "-o", "/dev/null", "-w", "%{http_code}", "-X", "POST",
        "-H", "content-type: application/json",
        "-d", r#"{"key":"k","value":"v"}"#, &kv_url,
    ])
    .await;
    assert_eq!(code, "401", "unauthenticated write over TLS should be 401");

    // Write with the token → 200.
    let (_ok, code) = curl(&[
        "-sk", "-o", "/dev/null", "-w", "%{http_code}", "-X", "POST",
        "-H", "content-type: application/json",
        "-H", "authorization: Bearer tok",
        "-d", r#"{"key":"k","value":"v"}"#, &kv_url,
    ])
    .await;
    assert_eq!(code, "200", "authenticated write over TLS should be 200");

    // Plaintext HTTP to a TLS port must fail (no valid HTTP response).
    let http_url = format!("http://127.0.0.1:{}/status", port);
    let (ok, _code) = curl(&["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "3", &http_url]).await;
    assert!(!ok, "plaintext HTTP against a TLS listener should fail");
}
