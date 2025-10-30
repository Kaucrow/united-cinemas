use crate::prelude::*;

use anyhow::Result;
use base64::{
    prelude::BASE64_STANDARD,
    Engine,
};

use actix_web::{ rt, web, App, Error, HttpRequest, HttpResponse, HttpServer };
use actix_ws::AggregatedMessage;
use futures_util::StreamExt;
use serde_json::ser;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ClientPayload {
    pub action: String,
    pub name: String,
    pub sdp: String,
}


/// This message will be sent from the SignalingServer to the ws_handler via the ws_send channel
pub enum ServerToClientMsg {
    Text(String),
    Close,
}

/// This message will be sent from the ws_handler to the SignalingServer via the ws_recv channel
struct SdpMessage {
    payload: ClientPayload,
    // Used by the SignalingServer to send a response back to the ws_handler
    responder: oneshot::Sender<String>,
}

pub struct SignalingServer {
    ws_recv_rx: mpsc::Receiver<SdpMessage>,
    // Receiver to get the session's WebSocket message sender from the ws_handler
    ws_send_rx: mpsc::Receiver<mpsc::Sender<ServerToClientMsg>>, 
}

impl SignalingServer {
    pub async fn new(port: u16) -> Result<Self> {
        let (ws_recv_tx, ws_recv_rx) = mpsc::channel::<SdpMessage>(1);

        // Create a channel for receiving the active WS sender
        let (ws_send_tx, ws_send_rx) = mpsc::channel::<mpsc::Sender<ServerToClientMsg>>(1);

        // Inject the Session Sender transmitter into Actix app state
        let ws_recv_tx_data = web::Data::new(ws_recv_tx);
        let ws_send_tx_data = web::Data::new(ws_send_tx);

        tokio::spawn(async move {
            let server = HttpServer::new(move || {
                App::new()
                    .app_data(ws_recv_tx_data.clone())
                    .app_data(ws_send_tx_data.clone()) // Inject the new sender
                    .route("/ws", web::get().to(ws_handler))
            })
            .bind(("0.0.0.0", port))
            .map_err(|e| anyhow!("Failed to bind Actix-Web server: {}", e))?
            .run();

            if let Err(e) = server.await {
                error!("Actix-Web server error: {e}");
            }
            Ok::<(), anyhow::Error>(())
        });

        Ok(Self {
            ws_recv_rx,
            ws_send_rx,
        })
    }

    pub async fn wait_for_offer(
        &mut self,
    ) -> Result<(ClientPayload, oneshot::Sender<String>)> {
        let _sender = self.ws_send_rx.recv().await.unwrap();

        let msg = self.ws_recv_rx.recv().await.unwrap();

        // let desc_data = SignalingServer::decode(&msg.sdp)?;
        // let offer = serde_json::from_str::<RTCSessionDescription>(&msg.payload.sdp)?;

        Ok((msg.payload, msg.responder))
    }

    pub fn encode_sdp(&self, sdp: &RTCSessionDescription) -> Result<String> {
        let json_str = serde_json::to_string(sdp)?;
        Ok(SignalingServer::encode(&json_str))
    }

    pub fn decode_sdp(&self, encoded_sdp: &str) -> Result<RTCSessionDescription> {
        let desc_data = SignalingServer::decode(encoded_sdp)?;
        let sdp = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
        Ok(sdp)
    }

    fn encode(b: &str) -> String {
        BASE64_STANDARD.encode(b)
    }

    fn decode(s: &str) -> Result<String> {
        let b = BASE64_STANDARD.decode(s)?;
        let s = String::from_utf8(b)?;
        Ok(s)
    }
}

async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    ws_recv_tx: web::Data<mpsc::Sender<SdpMessage>>,
    // The ws_handler will use this to send its own message-sender to the SignalingServer
    ws_send_tx: web::Data<mpsc::Sender<mpsc::Sender<ServerToClientMsg>>>,
) -> Result<HttpResponse, Error> {

    let (res, mut session, stream) = actix_ws::handle(&req, stream)?;
    let mut stream = stream.aggregate_continuations().max_continuation_size(2_usize.pow(20));
    let ws_recv_tx = ws_recv_tx.get_ref().clone();
    let ws_send_tx = ws_send_tx.get_ref().clone();

    // Create a channel for the SignalingServer to send messages to this WebSocket session
    let (to_client_tx, mut to_client_rx) = mpsc::channel::<ServerToClientMsg>(10);

    // Send the to_client_tx channel to the SignalingServer thread
    // This allows the main SignalingServer thread to talk to this specific WebSocket connection
    if let Err(e) = ws_send_tx.send(to_client_tx).await {
        error!("Failed to register client sender with SignalingServer: {}", e);
        let _ = session.close(None).await;
        return Ok(res); // Return response but stop processing
    }

    // Spawn a new task to handle the message stream
    rt::spawn(async move {
        loop {
            tokio::select! {
                /* --- Handle incoming client messages (offers, candidates, etc.) --- */
                msg = stream.next() => {
                    match msg {
                        Some(Ok(AggregatedMessage::Text(text))) => {
                            match BASE64_STANDARD.decode(&text) {
                                Ok(raw) => match String::from_utf8(raw) {
                                    Ok(payload_json) => match serde_json::from_str::<ClientPayload>(&payload_json) {
                                        Ok(payload) => {
                                            let (resp_tx, resp_rx) = oneshot::channel::<String>();
                                            // SdpMessage expects a parsed payload (not the raw base64)
                                            let sdp_msg = SdpMessage { payload: payload.clone(), responder: resp_tx };

                                            if let Err(e) = ws_recv_tx.send(sdp_msg).await {
                                                error!("Failed to send SDP message to signaling server: {}", e);
                                                break;
                                            }

                                            match resp_rx.await {
                                                Ok(answer_sdp) => {
                                                    if let Err(e) = session.text(answer_sdp).await {
                                                        error!("Failed to send SDP answer to client: {}", e);
                                                    }
                                                    break; 
                                                }
                                                Err(e) => {
                                                    error!("Signaling server failed to provide an answer: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => { error!("Failed to parse ClientPayload JSON: {}", e); }
                                    },
                                    Err(e) => { error!("Failed to parse UTF-8 from decoded SDP: {}", e); }
                                },
                                Err(e) => { error!("Invalid base64 SDP received: {}", e); }
                            }
                        }
                        Some(Ok(AggregatedMessage::Ping(msg))) => {
                            if let Err(e) = session.pong(&msg).await { error!("Failed to send PONG: {e}"); break; }
                        }
                        Some(Ok(AggregatedMessage::Close(_))) | None => break, // Client closed or stream ended
                        Some(Err(e)) => { error!("WebSocket error: {e}"); break; }
                        _ => (), // Ignore other messages
                    }
                }

                /* --- Handle outgoing server messages --- */
                out_msg = to_client_rx.recv() => {
                    match out_msg {
                        Some(ServerToClientMsg::Text(text)) => {
                            if let Err(e) = session.text(text).await {
                                error!("Failed to send ServerToClientMsg::Text: {e}");
                                break;
                            }
                        }
                        Some(ServerToClientMsg::Close) | None => break, // Server requested close or channel closed
                    }
                }
            }
        }
 
        // Ensure the session is closed when the task ends
        let _ = session.close(None).await;
        // NOTE: The mpsc sender for SdpMessage is dropped, signaling to SignalingServer
    });

    Ok(res)
}