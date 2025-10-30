use anyhow::Result;
use united_cinemas::{
    prelude::*,
    settings::Settings,
    telemetry,
    components::*,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Build settings
    let settings = Settings::new();

    // Init the tracing subscriber
    let (subscriber, _guard) = telemetry::get_subscriber(&settings).await?;
    telemetry::init_subscriber(subscriber);

    let port = settings.port;

    // Init components
    let mut signaling = SignalingServer::new(port).await?;
    let peer_conn_factory = Arc::new(PeerConnectionFactory::new().await?);
    let session_manager = SessionManager::new(Arc::clone(&peer_conn_factory));
    let broadcast_manager = Arc::new(BroadcastManager::new());

    info!("Signaling server waiting for offer on localhost:{}/sdp", port);
    info!("Waiting for broadcasters and viewers...");

    loop {
        // Wait for any client connection (broadcaster or viewer)
        let (payload, responder) = signaling.wait_for_offer().await?;
        
        match payload.action.as_str() {
            "broadcast" => {
                info!("New broadcast request: '{}'", payload.name);
                
                // Create a dedicated track manager for this broadcaster
                let mut track_manager = TrackManager::new();
                
                // Decode the SDP offer from the broadcaster
                let offer = signaling.decode_sdp(&payload.sdp)?;
                
                // Create a WebRTC session to receive video from the broadcaster
                let peer_connection = session_manager
                    .create_broadcaster_session(offer, &mut track_manager)
                    .await?;
                
                // Create and send the SDP answer back to the broadcaster
                let local_desc = session_manager.create_answer(&peer_connection).await?;
                let response = signaling.encode_sdp(&local_desc)?;
                let _ = responder.send(response);
                
                info!("SDP answer sent to broadcaster '{}'", payload.name);
                
                // Wait for the video track to arrive, then register the broadcast
                let broadcast_name = payload.name.clone();
                let broadcast_manager_clone = Arc::clone(&broadcast_manager);
                
                tokio::spawn(async move {
                    if let Some(local_track) = track_manager.get_track_receiver().recv().await {
                        broadcast_manager_clone.register_broadcast(
                            broadcast_name.clone(),
                            Arc::clone(&local_track),
                            Arc::clone(&peer_connection)
                        ).await;
                        
                        info!("Broadcast '{}' is ready for viewers", broadcast_name);
                    } else {
                        warn!("No track received for broadcast '{}'", broadcast_name);
                    }
                });
            }
            
            "join" => {
                info!("Viewer wants to join broadcast: '{}'", payload.name);
                
                // Look up the broadcast in the registry
                if let Some(local_track) = broadcast_manager.get_broadcast(&payload.name).await {
                    // Decode the SDP offer from the viewer
                    let offer = signaling.decode_sdp(&payload.sdp)?;
                    
                    // Create a WebRTC session to send video to the viewer
                    let peer_connection = session_manager
                        .create_viewer_session(offer, Arc::clone(&local_track))
                        .await?;
                    
                    // Create and send the SDP answer back to the viewer
                    let local_desc = session_manager.create_answer(&peer_connection).await?;
                    let response = signaling.encode_sdp(&local_desc)?;
                    let _ = responder.send(response);
                    
                    info!("Viewer connected to broadcast '{}'", payload.name);
                } else {
                    warn!("Broadcast '{}' not found!", payload.name);
                    // TODO: should probably send an error back to the client here
                }
            }
            
            _ => {
                warn!("Unknown action: '{}' from client", payload.action);
            }
        }
    }
}
