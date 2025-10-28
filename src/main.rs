use anyhow::Result;
use tokio::time::Duration;
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
    let peer_conn_factory = PeerConnectionFactory::new().await?;
    let mut track_manager = TrackManager::new();

    // Wait for the offer
    info!("Signaling server waiting for offer on localhost:{}/sdp", port);
    let offer = signaling.wait_for_offer().await?;

    // Create a new RTCPeerConnection
    let peer_connection = peer_conn_factory.create_peer_connection().await?;

    // Allow us to receive 1 video track
    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, None)
        .await?;

    let _ = track_manager.setup_track_handlers(Arc::clone(&peer_connection));

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
        println!("Peer Connection State has changed: {s}");
        Box::pin(async {})
    }));

    // Set the remote SessionDescription
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    if let Some(local_desc) = peer_connection.local_description().await {
        let local_desc = signaling.encode_sdp(&local_desc)?;
        println!("{local_desc}");
    } else {
        println!("generate local_description failed!");
    }

    if let Some(local_track) = track_manager.get_track_receiver().recv().await {
        loop {
            println!("\nCurl an base64 SDP to start sendonly peer connection");

            let recv_only_offer = signaling.wait_for_offer().await?;

            // Create a MediaEngine object to configure the supported codec
            let mut m = MediaEngine::default();

            m.register_default_codecs()?;

            // Create a new RTCPeerConnection
            let peer_connection = peer_conn_factory
                .create_recv_only_peer_connection(Arc::clone(&local_track)).await?;

            // Set the handler for Peer connection state
            // This will notify you when the peer has connected/disconnected
            peer_connection.on_peer_connection_state_change(Box::new(
                move |s: RTCPeerConnectionState| {
                    println!("Peer Connection State has changed: {s}");
                    Box::pin(async {})
                },
            ));

            // Set the remote SessionDescription
            peer_connection
                .set_remote_description(recv_only_offer)
                .await?;

            // Create an answer
            let answer = peer_connection.create_answer(None).await?;

            // Create channel that is blocked until ICE Gathering is complete
            let mut gather_complete = peer_connection.gathering_complete_promise().await;

            // Sets the LocalDescription, and starts our UDP listeners
            peer_connection.set_local_description(answer).await?;

            // Block until ICE Gathering is complete, disabling trickle ICE
            // we do this because we only can exchange one signaling message
            // in a production application you should exchange ICE Candidates via OnICECandidate
            let _ = gather_complete.recv().await;

            if let Some(local_desc) = peer_connection.local_description().await {
                let local_desc = signaling.encode_sdp(&local_desc)?;
                println!("{local_desc}");
            } else {
                println!("generate local_description failed!");
            }
        }
    }

    Ok(())
}