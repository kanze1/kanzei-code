//! One connection owns all OneBot calls. Reconnection never replays a mutation.
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_util::sync::CancellationToken;

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
#[derive(Debug, Clone)]
pub(crate) enum Failure {
    Unavailable(String),
    Rejected(String),
    Unknown(String),
}
impl Failure {
    pub fn state(&self) -> &'static str {
        match self {
            Self::Unknown(_) => "unknown",
            _ => "failed",
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (Self::Unavailable(message) | Self::Rejected(message) | Self::Unknown(message)) = self;
        f.write_str(message)
    }
}
impl From<Failure> for String {
    fn from(failure: Failure) -> Self {
        failure.to_string()
    }
}
type Reply = oneshot::Sender<Result<Value, Failure>>;
struct Call {
    action: String,
    params: Value,
    echo: String,
    reply: Reply,
}

#[derive(Clone)]
pub(crate) struct Client {
    tx: mpsc::Sender<Call>,
    sequence: Arc<AtomicU64>,
}
impl Client {
    pub async fn call(&self, action: &str, params: Value) -> Result<Value, Failure> {
        let (reply, result) = oneshot::channel();
        let call = Call {
            action: action.into(),
            params,
            echo: self.sequence.fetch_add(1, Ordering::Relaxed).to_string(),
            reply,
        };
        self.tx
            .send(call)
            .await
            .map_err(|_| Failure::Unavailable("QQ 连接已停止，未发送".into()))?;
        tokio::time::timeout(CALL_TIMEOUT, result)
            .await
            .map_err(|_| Failure::Unknown("QQ 接口响应超时，结果未知；未自动重发".into()))?
            .map_err(|_| Failure::Unknown("QQ 连接中断，结果未知；未自动重发".into()))?
    }
}

pub(crate) enum Event {
    Connected(Value),
    Incoming(Value),
    Offline(String),
}

pub(crate) fn start(
    url: String,
    token: String,
    mut account: String,
    cancel: CancellationToken,
    events: mpsc::Sender<Event>,
) -> (Client, tokio::task::JoinHandle<()>) {
    let (tx, mut calls) = mpsc::channel::<Call>(64);
    let client = Client {
        tx,
        sequence: Arc::new(AtomicU64::new(1)),
    };
    let task = tokio::spawn(async move {
        let mut delay = Duration::from_secs(1);
        loop {
            let result = socket(
                &url,
                &token,
                &mut account,
                &mut delay,
                &cancel,
                &events,
                &mut calls,
            )
            .await;
            if cancel.is_cancelled() {
                break;
            }
            if events
                .send(Event::Offline(
                    result.err().unwrap_or_else(|| "QQ 连接已关闭".into()),
                ))
                .await
                .is_err()
            {
                break;
            }
            let reconnect = tokio::time::sleep(delay);
            tokio::pin!(reconnect);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = &mut reconnect => break,
                    call = calls.recv() => match call { Some(call) => { let _ = call.reply.send(Err(Failure::Unavailable("QQ 当前未连接，未发送请求".into()))); }, None => return }
                }
            }
            delay = (delay * 2).min(Duration::from_secs(30));
        }
    });
    (client, task)
}

async fn socket(
    url: &str,
    token: &str,
    account: &mut String,
    reconnect_delay: &mut Duration,
    cancel: &CancellationToken,
    events: &mpsc::Sender<Event>,
    calls: &mut mpsc::Receiver<Call>,
) -> Result<(), String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| "OneBot 令牌格式无效")?,
    );
    let connection = tokio::select! {
        _ = cancel.cancelled() => return Ok(()),
        connection = tokio::time::timeout(CALL_TIMEOUT, tokio_tungstenite::connect_async(request)) => connection.map_err(|_| "OneBot 连接超时")?.map_err(|e| e.to_string())?
    };
    let (mut writer, mut reader) = connection.0.split();
    writer
        .send(Message::Text(
            json!({"action":"get_login_info","params":{},"echo":"login"})
                .to_string()
                .into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
    let mut pending: HashMap<String, Reply> = HashMap::new();
    let login_deadline = tokio::time::sleep(CALL_TIMEOUT);
    tokio::pin!(login_deadline);
    let mut authenticated = false;
    let outcome = async {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => { let _ = writer.close().await; return Ok(()); },
                _ = &mut login_deadline, if !authenticated => return Err("OneBot 登录身份响应超时".into()),
                call = calls.recv() => {
                    let Some(call) = call else { return Ok(()); };
                    if !authenticated || call.reply.is_closed() {
                        let _ = call.reply.send(Err(Failure::Unavailable("QQ 尚未确认登录身份，未发送请求".into())));
                        continue;
                    }
                    pending.retain(|_, reply| !reply.is_closed());
                    let frame = json!({"action":call.action,"params":call.params,"echo":call.echo});
                    pending.insert(call.echo, call.reply);
                    writer.send(Message::Text(frame.to_string().into())).await.map_err(|e| e.to_string())?;
                },
                frame = reader.next() => {
                    let frame = frame.ok_or("OneBot 连接关闭")?.map_err(|e| e.to_string())?;
                    match frame {
                        Message::Text(text) => {
                            let value: Value = serde_json::from_str(&text).map_err(|e| format!("OneBot 非法消息: {e}"))?;
                            if let Some(echo) = value["echo"].as_str() {
                                let result = response(&value);
                                if echo == "login" {
                                    let info = result.map_err(String::from)?;
                                    let logged_in = id(&info["user_id"])?;
                                    if !account.is_empty() && *account != logged_in {
                                        return Err("NapCat 登录账号与配置不一致，未接受消息或请求".into());
                                    }
                                    *account = logged_in;
                                    *reconnect_delay = Duration::from_secs(1);
                                    events.send(Event::Connected(info)).await.map_err(|_| "QQ 消费者已退出")?;
                                    authenticated = true;
                                }
                                else if let Some(reply) = pending.remove(echo) { let _ = reply.send(result); }
                            } else if authenticated && value["post_type"].as_str() == Some("message") {
                                events.send(Event::Incoming(value)).await.map_err(|_| "QQ 消费者已退出")?;
                            }
                        },
                        Message::Ping(data) => writer.send(Message::Pong(data)).await.map_err(|e| e.to_string())?,
                        Message::Close(_) => return Err("OneBot 连接关闭".into()),
                        _ => {}
                    }
                }
            }
        }
    }.await;
    for (_, reply) in pending {
        let _ = reply.send(Err(Failure::Unknown(
            "QQ 连接中断，结果未知；未自动重发".into(),
        )));
    }
    outcome
}

fn response(value: &Value) -> Result<Value, Failure> {
    if value["status"] == "ok" && value["retcode"].as_i64() == Some(0) {
        Ok(value["data"].clone())
    } else {
        let message = format!(
            "OneBot 请求失败 (retcode={}): {}",
            value["retcode"], value["wording"]
        );
        if value["status"] == "failed" {
            Err(Failure::Rejected(message))
        } else {
            Err(Failure::Unknown(message))
        }
    }
}

pub(crate) fn id(value: &Value) -> Result<String, String> {
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return Err("QQ 身份缺失".into()),
    };
    let number: u64 = text.parse().map_err(|_| "QQ 身份无效")?;
    if number == 0 || number.to_string() != text {
        return Err("QQ 身份无效".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires logged-in NapCat and KANZEI_QQ_URL, KANZEI_QQ_TOKEN, KANZEI_QQ_ACCOUNT"]
    async fn live_napcat_identity_and_directory_contract() {
        let url = std::env::var("KANZEI_QQ_URL").expect("KANZEI_QQ_URL is required");
        let token = std::env::var("KANZEI_QQ_TOKEN").expect("KANZEI_QQ_TOKEN is required");
        let account = std::env::var("KANZEI_QQ_ACCOUNT").expect("KANZEI_QQ_ACCOUNT is required");
        let cancel = CancellationToken::new();
        let (tx, mut events) = mpsc::channel(128);
        let (client, task) = start(url, token, account.clone(), cancel.clone(), tx);
        let result = tokio::time::timeout(CALL_TIMEOUT, async {
            match events.recv().await.expect("OneBot event channel") {
                Event::Connected(info) => assert_eq!(id(&info["user_id"]).unwrap(), account),
                Event::Offline(error) => panic!("NapCat did not connect: {error}"),
                Event::Incoming(_) => panic!("message arrived before login identity"),
            }
            assert!(client
                .call("get_friend_list", json!({}))
                .await
                .unwrap()
                .is_array());
            assert!(client
                .call("get_group_list", json!({}))
                .await
                .unwrap()
                .is_array());
        })
        .await;
        cancel.cancel();
        task.await.unwrap();
        result.expect("NapCat acceptance timed out");
    }
    // Tungstenite owns this callback's response/error representation.
    #[allow(clippy::result_large_err)]
    fn authenticate(
        request: &tokio_tungstenite::tungstenite::handshake::server::Request,
        response: tokio_tungstenite::tungstenite::handshake::server::Response,
    ) -> Result<
        tokio_tungstenite::tungstenite::handshake::server::Response,
        tokio_tungstenite::tungstenite::handshake::server::ErrorResponse,
    > {
        assert_eq!(request.headers()["Authorization"], "Bearer secret");
        Ok(response)
    }
    #[tokio::test]
    async fn different_login_account_never_authenticates_the_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.next().await.unwrap().unwrap();
            ws.send(Message::Text(
                json!({"post_type":"message","raw_message":"unverified"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text(
                json!({"status":"ok","retcode":0,"echo":"login","data":{"user_id":999}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        });
        let cancel = CancellationToken::new();
        let (tx, mut events) = mpsc::channel(8);
        let (client, worker) = start(url, "secret".into(), "123".into(), cancel.clone(), tx);
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap();
        assert!(matches!(event,Some(Event::Offline(message)) if message.contains("账号")));
        assert!(matches!(
            client.call("send_group_msg", json!({})).await,
            Err(Failure::Unavailable(_))
        ));
        cancel.cancel();
        worker.await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_auth_events_rejections_and_disconnect_never_replay_mutations() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(stream, authenticate)
                .await
                .unwrap();
            let login: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(login["action"], "get_login_info");
            ws.send(Message::Text(json!({"status":"ok","retcode":0,"echo":"login","data":{"user_id":123,"nickname":"bot"}}).to_string().into())).await.unwrap();
            ws.send(Message::Text(json!({"post_type":"message","message_type":"group","group_id":456,"raw_message":"ordinary non-mention message"}).to_string().into())).await.unwrap();
            let call: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(call["action"], "send_group_msg");
            ws.send(Message::Text(
                json!({"status":"failed","retcode":100,"echo":call["echo"],"wording":"denied"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            let mutation: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(mutation["action"], "send_private_msg");
            ws.close(None).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let login: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                login["action"], "get_login_info",
                "only identity is re-requested after reconnect"
            );
            ws.send(Message::Text(
                json!({"status":"ok","retcode":0,"echo":"login","data":{"user_id":123}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            // The first post-reconnect request must be this explicit read, never the old mutation.
            let read: Value =
                serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(read["action"], "get_friend_list");
            ws.send(Message::Text(
                json!({"status":"ok","retcode":0,"echo":read["echo"],"data":[]})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        });
        let cancel = CancellationToken::new();
        let (tx, mut events) = mpsc::channel(16);
        let (client, worker) = start(url, "secret".into(), "123".into(), cancel.clone(), tx);
        let scenario = async {
            assert!(matches!(events.recv().await, Some(Event::Connected(_))));
            assert!(matches!(events.recv().await, Some(Event::Incoming(_))));
            assert!(matches!(
                client.call("send_group_msg", json!({"group_id":456})).await,
                Err(Failure::Rejected(_))
            ));
            assert!(matches!(
                client.call("send_private_msg", json!({"user_id":7})).await,
                Err(Failure::Unknown(_))
            ));
            assert!(matches!(events.recv().await, Some(Event::Offline(_))));
            assert!(matches!(events.recv().await, Some(Event::Connected(_))));
            assert_eq!(
                client.call("get_friend_list", json!({})).await.unwrap(),
                json!([])
            );
            server.await.unwrap();
        };
        tokio::time::timeout(Duration::from_secs(10), scenario)
            .await
            .unwrap();
        cancel.cancel();
        worker.await.unwrap();
    }
    #[test]
    fn failure_and_async_acceptance_are_not_delivery_success() {
        assert!(response(&json!({"status":"ok","retcode":1})).is_err());
        assert!(response(&json!({"status":"failed","retcode":0})).is_err());
        assert_eq!(
            response(&json!({"status":"ok","retcode":0,"data":{"message_id":9}})).unwrap()
                ["message_id"],
            9
        );
    }
}
