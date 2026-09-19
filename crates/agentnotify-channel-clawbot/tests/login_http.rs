use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use agentnotify_channel_clawbot::{ClawBotAuthTransport, ClawBotHttpClient};
use agentnotify_channel_sdk::ChannelError;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn fetch_qr_code_posts_base_info_and_local_tokens() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url = spawn_server(
        r#"{"qrcode":"qr-1","qrcode_img_content":"","ret":0,"errcode":0}"#,
        capture.clone(),
    )
    .await;
    let client = ClawBotHttpClient::new().unwrap();

    let response = client
        .fetch_qr_code(&base_url, &["token-1".into()])
        .await
        .unwrap();

    assert_eq!(response.qrcode, "qr-1");
    let request = capture.lock().expect("请求捕获锁不应失败").clone();
    assert!(request.starts_with("POST /ilink/bot/get_bot_qrcode?bot_type=3 "));
    assert!(request.contains("\"local_token_list\":[\"token-1\"]"));
    assert!(request.contains("\"channel_version\":\"2.4.6\""));
    assert!(request.contains("ilink-app-id: bot"));
    assert!(request.contains("x-wechat-uin:"));
}

#[tokio::test]
async fn poll_qr_status_sends_verification_code_as_query_parameter() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url = spawn_server(
        r#"{"status":"confirmed","bot_token":"token","ilink_bot_id":"bot","ilink_user_id":"user","baseurl":"https://business.example.test","ret":0}"#,
        capture.clone(),
    )
    .await;
    let client = ClawBotHttpClient::new().unwrap();

    let response = client
        .poll_qr_status(&base_url, "qr with space", Some("123456"))
        .await
        .unwrap();

    assert_eq!(response.status, "confirmed");
    let request = capture.lock().expect("请求捕获锁不应失败").clone();
    assert!(
        request.starts_with(
            "GET /ilink/bot/get_qrcode_status?qrcode=qr+with+space&verify_code=123456 "
        )
    );
}

#[tokio::test]
async fn invalid_account_status_maps_to_invalid_account() {
    let capture = Arc::new(Mutex::new(String::new()));
    let base_url = spawn_server(r#"{"status":"wait","ret":-14,"errcode":-14}"#, capture).await;
    let client = ClawBotHttpClient::new().unwrap();

    let error = client
        .poll_qr_status(&base_url, "qr-1", None)
        .await
        .unwrap_err();

    assert!(matches!(error, ChannelError::InvalidAccount(_)));
}

async fn spawn_server(response_body: &'static str, capture: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = index + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let read = stream.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        *capture.lock().unwrap() = String::from_utf8_lossy(&request).into_owned();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if address.port() != 0 {
                return format!("http://{address}");
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
