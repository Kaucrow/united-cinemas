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

    let host = settings.host;
    let port = settings.port;

    // Init components
    let mut signaling = SignalingServer::new(host.clone(), port).await?;
    let peer_conn_factory = Arc::new(PeerConnectionFactory::new().await?);
    let session_manager = SessionManager::new(Arc::clone(&peer_conn_factory));
    let broadcast_manager = Arc::new(BroadcastManager::new());

    info!("Signaling server waiting for offer via WebSocket connection on ws://{}:{}/ws", host, port);

    loop {
        // Wait for any client connection (broadcaster or viewer)
        let (payload, responder) = signaling.wait_for_offer().await?;
        let broadcast = payload.name;

        match payload.action.as_str() {
            "broadcast" => {
                info!("Broadcast '{}': New broadcaster request", broadcast);

                // Create a dedicated track manager for this broadcaster
                let mut track_manager = TrackManager::new(broadcast.clone());

                // Decode the SDP offer from the broadcaster
                let offer = signaling.decode_sdp(&payload.sdp)?;
                debug!("Broadcast '{}': SDP offer decoded successfully", broadcast);
 
                // Create a WebRTC session to receive video from the broadcaster
                let peer_connection = session_manager
                    .create_broadcaster_session(broadcast.clone(), offer, &mut track_manager)
                    .await?;
                debug!("Broadcast '{}': WebRTC session created for broadcaster", broadcast);
 
                // Create and send the SDP answer back to the broadcaster
                let local_desc = session_manager.create_answer(&peer_connection).await?;
                let response = signaling.encode_sdp(&local_desc)?;
                let _ = responder.send(response);
                
                info!("Broadcast '{}': SDP answer sent to broadcaster", broadcast);
 
                // Wait for the video track to arrive, then register the broadcast
                let broadcast_name = broadcast.clone();
                let broadcast_manager_clone = Arc::clone(&broadcast_manager);

                tokio::spawn(async move {
                    debug!("Broadcast '{}': Waiting for video track from broadcaster", broadcast_name);
 
                    if let Some(local_track) = track_manager.get_track_receiver().recv().await {
                        debug!("Broadcast '{}': Video track received, registering broadcast", broadcast_name);
 
                        broadcast_manager_clone.register_broadcast(
                            broadcast_name.clone(),
                            Arc::clone(&local_track),
                        ).await;

                        info!("Broadcast '{}': Ready for viewers", broadcast_name);
                    } else {
                        debug!("Broadcast '{}': Failed to receive video track from broadcaster", broadcast_name);
                    }
                });
            }
 
            "join" => {
                info!("Broadcast '{}': Viewer wants to join broadcast", broadcast);
                
                // Look up the broadcast in the registry
                if let Some(local_track) = broadcast_manager.get_broadcast(&broadcast).await {
                    debug!("Broadcast '{}': Broadcast found in registry", broadcast);
 
                    // Decode the SDP offer from the viewer
                    let offer = signaling.decode_sdp(&payload.sdp)?;
                    debug!("Broadcast '{}': Viewer SDP offer decoded", broadcast);
 
                    // Create a WebRTC session to send video to the viewer
                    let peer_connection = session_manager
                        .create_viewer_session(broadcast.clone(), offer, Arc::clone(&local_track))
                        .await?;
                    debug!("Broadcast '{}': WebRTC session created for viewer", broadcast);
 
                    // Create and send the SDP answer back to the viewer
                    let local_desc = session_manager.create_answer(&peer_connection).await?;
                    let response = signaling.encode_sdp(&local_desc)?;
                    let _ = responder.send(response);

                    info!("Broadcast '{}': Viewer connected", broadcast);
                } else {
                    debug!("Broadcast '{}': Broadcast not found in registry", broadcast);
                    // TODO: should send an error back to the client here
                }
            }

            _ => {
                debug!("Unknown action '{}': Invalid action received from client", payload.action);
            }
        }
    }
}