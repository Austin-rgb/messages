use crate::models::{AppState, Receipt};
use actix::{
    Actor, ActorContext, AsyncContext, Context, Handler, Message, Recipient, StreamHandler,
};
use actix_web::{Error, HttpRequest, HttpResponse, get, web};
use actix_web_actors::ws;
use auth_middleware::UserContext;

use redis::{AsyncCommands, Client};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Message)]
#[rtype(result = "()")]
pub struct Connect {
    pub user_id: String,
    pub addr: Recipient<ServerMessage>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct DeliverMessage {
    pub to: String,
    pub payload: String,
    pub id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Disconnect {
    pub user_id: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct PrivateMessage {
    pub from: String,
    pub to: String,
    pub content: String,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct ServerMessage {
    pub payload: String,
    id: String,
}

pub struct ChatServer {
    users: HashMap<String, Recipient<ServerMessage>>,
}

impl ChatServer {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }
}

impl Actor for ChatServer {
    type Context = Context<Self>;
}

impl Handler<Connect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Connect, _: &mut Context<Self>) {
        self.users.insert(msg.user_id, msg.addr);
    }
}

impl Handler<Disconnect> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
        self.users.remove(&msg.user_id);
    }
}

impl Handler<PrivateMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: PrivateMessage, _: &mut Context<Self>) {
        if let Some(recipient) = self.users.get(&msg.to) {
            let payload = serde_json::json!({
                "from": msg.from,
                "content": msg.content
            });
            let _ = recipient.do_send(ServerMessage {
                payload: payload.to_string(),
                id: Uuid::new_v4().to_string(),
            });
        }
    }
}

impl Handler<DeliverMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: DeliverMessage, _: &mut Context<Self>) {
        if let Some(recipient) = self.users.get(&msg.to) {
            let _ = recipient.do_send(ServerMessage {
                payload: msg.payload,
                id: msg.id.to_string(),
            });
        }
    }
}

pub struct WsSession {
    user_id: String,
    server: actix::Addr<ChatServer>,
    redis: Client,
    last_heartbeat: Instant,
}

impl WsSession {
    fn start_heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(Duration::from_secs(5), |act, ctx| {
            // If client hasn't responded in 10s → disconnect
            if Instant::now().duration_since(act.last_heartbeat) > Duration::from_secs(10) {
                println!("WebSocket heartbeat failed, disconnecting {}", act.user_id);
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });
    }
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.last_heartbeat = Instant::now();
        self.start_heartbeat(ctx);

        let addr = ctx.address().recipient();
        self.server.do_send(Connect {
            user_id: self.user_id.clone(),
            addr,
        });
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        self.server.do_send(Disconnect {
            user_id: self.user_id.clone(),
        });
    }
}

#[derive(Deserialize)]
struct ClientMessage {
    #[serde(rename = "type")]
    msg_type: String,
    to: String,
    content: String,
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.last_heartbeat = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_heartbeat = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                if let Ok(parsed) = serde_json::from_str::<ClientMessage>(&text) {
                    if parsed.msg_type == "private" {
                        self.server.do_send(PrivateMessage {
                            from: self.user_id.clone(),
                            to: parsed.to,
                            content: parsed.content,
                        });
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => (),
        }
    }
}

impl Handler<ServerMessage> for WsSession {
    type Result = ();

    fn handle(&mut self, msg: ServerMessage, ctx: &mut Self::Context) {
        ctx.text(msg.payload);
        let user_id = self.user_id.clone();
        let redis = self.redis.clone();
        let msg_id = msg.id.clone();
        actix::spawn(async move {
            let mut conn = match redis.get_async_connection().await {
                Ok(c) => c,
                Err(_) => return,
            };

            let event = Receipt {
                message_id: msg_id,
                user_id,
                delivered: true,
                read: false,
                reaction: None,
                ts: chrono::Utc::now().timestamp(),
            };

            let payload = serde_json::to_string(&event).unwrap();

            let _: redis::RedisResult<String> = conn
                .xadd("receipts_stream", "*", &[("payload", payload)])
                .await;
        });
    }
}

#[get("/ws/")]
pub async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
    claims: web::ReqData<UserContext>,
) -> Result<HttpResponse, Error> {
    let session = WsSession {
        user_id: claims.username.clone(),
        server: state.get_ref().chat_server.clone(),
        last_heartbeat: Instant::now(),
        redis: state.redis.clone(),
    };

    let res = ws::start(session, &req, stream);
    match &res {
        Ok(r) => println!("ws response: {}", r.status()),
        Err(e) => println!("ws error: {}", e),
    }
    res
}
