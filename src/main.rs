use anyhow::Result;
use united_cinemas::{
    prelude::*,
    settings::Settings,
    telemetry,
    components::*
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
    let mut track_manager = TrackManager::new();
    let session_manager = SessionManager::new(Arc::clone(&peer_conn_factory));

    // Wait for the offer
    info!("Signaling server waiting for offer on localhost:{}/sdp", port);
    let broadcaster_offer = signaling.wait_for_offer().await?;

    // Allow us to receive 1 video track
    let peer_connection = session_manager.create_broadcaster_session(broadcaster_offer, &mut track_manager).await?;

    // Create the session description
    let local_desc = session_manager.create_answer(&peer_connection).await?;

    // Output the answer in base64 so we can paste it in browser
    let answer = signaling.encode_sdp(&local_desc)?;
    println!("{answer}");

    if let Some(local_track) = track_manager.get_track_receiver().recv().await {
        loop {
            println!("\nCurl an base64 SDP to start sendonly peer connection");

            let viewer_offer = signaling.wait_for_offer().await?;

            // Create a new RTCPeerConnection
            let peer_connection = session_manager.create_viewer_session(viewer_offer, Arc::clone(&local_track)).await?;

            // Create the session description
            let local_desc = session_manager.create_answer(&peer_connection).await?;

            // Output the answer in base64 so we can paste it in browser
            let answer = signaling.encode_sdp(&local_desc)?;
            println!("{answer}");
        }
    }

    Ok(())
}