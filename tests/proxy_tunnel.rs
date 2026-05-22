/// Verifies reqwest can complete an HTTPS request through a CONNECT proxy.
/// Reproduces the `native-tls` failure (`OSStatus -26276`) that occurs when
/// `https_proxy` routes traffic through an HTTP CONNECT proxy on macOS.
/// Skipped when no HTTPS proxy is configured.
#[tokio::test]
async fn https_through_connect_proxy() {
    let Ok(proxy_url) = std::env::var("https_proxy").or_else(|_| std::env::var("HTTPS_PROXY"))
    else {
        return;
    };

    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(&proxy_url).expect("proxy url"))
        .build()
        .expect("client build");

    let response = client
        .get("https://github.com")
        .send()
        .await
        .expect("request through proxy");

    assert!(response.status().is_success() || response.status().is_redirection());
}
