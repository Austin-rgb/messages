use actix::{Actor, ActorContext, Context, Handler, Message, Recipient, StreamHandler, AsyncContext};
use actix_web::{ HttpRequest, HttpResponse, Error, web, get};
use actix_web_actors::ws;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration,Instant};
use auth_middleware::common::Claims;
use crate::models::AppState;

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
pub struct ServerMessage(pub String);

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
            let _ = recipient.do_send(ServerMessage(payload.to_string()));
        }
    }
}

impl Handler<DeliverMessage> for ChatServer {
    type Result = ();

    fn handle(&mut self, msg: DeliverMessage, _: &mut Context<Self>) {
        if let Some(recipient) = self.users.get(&msg.to) {
            let _ = recipient.do_send(ServerMessage(msg.payload));
        }
    }
}


pub struct WsSession {
    user_id: String,
    server: actix::Addr<ChatServer>,
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
        ctx.text(msg.0);
    }
}

#[get("/ws/")]
pub async fn ws_route(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<AppState>,
claims:web::ReqData<Claims>,
) -> Result<HttpResponse, Error> {
    
    let session = WsSession {
        user_id:claims.sub.clone(),
        server: state.get_ref().chat_server.clone(),
        last_heartbeat: Instant::now(),
    };

    ws::start(session, &req, stream)
}
