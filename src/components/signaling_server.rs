use crate::prelude::*;
use anyhow::Result;
use lazy_static::lazy_static;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use tokio::sync::{mpsc, Mutex};
lazy_static! {
    static ref SDP_CHAN_TX_MUTEX: Arc<Mutex<Option<mpsc::Sender<String>>>> =
        Arc::new(Mutex::new(None));
}

pub struct SignalingServer {
    port: u16,
    sdp_chan_rx: mpsc::Receiver<String>,
}

impl SignalingServer {
    pub async fn new(port: u16) -> Result<Self> {
        let (sdp_chan_tx, sdp_chan_rx) = mpsc::channel::<String>(1);
        {
            let mut tx = SDP_CHAN_TX_MUTEX.lock().await;
            *tx = Some(sdp_chan_tx);
        }

        tokio::spawn(async move {
            let addr = SocketAddr::from_str(&format!("0.0.0.0:{port}")).unwrap();
            let service =
                make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(SignalingServer::remote_handler)) });
            let server = Server::bind(&addr).serve(service);
            if let Err(e) = server.await {
                error!("Server error: {e}");
            }
        });

        Ok(Self {
            port,
            sdp_chan_rx
        })
    }

    async fn remote_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        match (req.method(), req.uri().path()) {
            // An HTTP handler that processes a SessionDescription given to us from the other WebRTC-rs or Pion process
            (&Method::POST, "/sdp") => {
                //println!("remote_handler receive from /sdp");
                let sdp_str = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
                {
                    Ok(s) => s.to_owned(),
                    Err(err) => panic!("{}", err),
                };

                {
                    let sdp_chan_tx = SDP_CHAN_TX_MUTEX.lock().await;
                    if let Some(tx) = &*sdp_chan_tx {
                        let _ = tx.send(sdp_str).await;
                    }
                }

                let mut response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::OK;
                Ok(response)
            }
            // Return the 404 Not Found for other routes.
            _ => {
                let mut not_found = Response::default();
                *not_found.status_mut() = StatusCode::NOT_FOUND;
                Ok(not_found)
            }
        }
    }

    /// encode encodes the input in base64
    /// It can optionally zip the input before encoding
    fn encode(b: &str) -> String {
        BASE64_STANDARD.encode(b)
    }

    /// decode decodes the input from base64
    /// It can optionally unzip the input after decoding
    fn decode(s: &str) -> Result<String> {
        let b = BASE64_STANDARD.decode(s)?;

        let s = String::from_utf8(b)?;
        Ok(s)
    }

    pub async fn wait_for_offer(&mut self) -> Result<RTCSessionDescription> {
        let sdp_chan_rx = &mut self.sdp_chan_rx;
        let line = sdp_chan_rx.recv().await.unwrap();
        let desc_data = SignalingServer::decode(line.as_str())?;
        let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
        Ok(offer)
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
}