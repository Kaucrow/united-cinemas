use anyhow::Result;
use tokio::time::Duration;
use united_cinemas::{
    prelude::*,
    settings::Settings,
    telemetry,
    components::signaling_server::SignalingServer,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Build settings
    let settings = Settings::new();

    // Init the tracing subscriber
    let (subscriber, _guard) = telemetry::get_subscriber(&settings).await?;
    telemetry::init_subscriber(subscriber);

    let port = settings.port;
    let mut signaling = SignalingServer::new(port).await?;

    // Wait for the offer
    info!("Signaling server waiting for offer on localhost:{}/sdp", port);
    let offer = signaling.wait_for_offer().await?;
    //println!("Receive offer from http_sdp_server:\n{:?}", offer);

    // Everything below is the WebRTC-rs API! Thanks for using it ❤️.

    // Create a MediaEngine object to configure the supported codec
    let mut m = MediaEngine::default();

    m.register_default_codecs()?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut m)?;

    // Create the API object with the MediaEngine
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Prepare the configuration
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    // Create a new RTCPeerConnection
    let peer_connection = Arc::new(api.new_peer_connection(config).await?);

    // Allow us to receive 1 video track
    peer_connection
        .add_transceiver_from_kind(RTPCodecType::Video, None)
        .await?;

    let (local_track_chan_tx, mut local_track_chan_rx) =
        tokio::sync::mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);

    let local_track_chan_tx = Arc::new(local_track_chan_tx);
    // Set a handler for when a new remote track starts, this handler copies inbound RTP packets,
    // replaces the SSRC and sends them back
    let pc = Arc::downgrade(&peer_connection);
    peer_connection.on_track(Box::new(move |track, _, _| {
        // Send a PLI on an interval so that the publisher is pushing a keyframe every rtcpPLIInterval
        // This is a temporary fix until we implement incoming RTCP events, then we would push a PLI only when a viewer requests it
        let media_ssrc = track.ssrc();
        let pc2 = pc.clone();
        tokio::spawn(async move {
            let mut result = Result::<usize>::Ok(0);
            while result.is_ok() {
                let timeout = tokio::time::sleep(Duration::from_secs(3));
                tokio::pin!(timeout);

                tokio::select! {
                    _ = timeout.as_mut() =>{
                        if let Some(pc) = pc2.upgrade(){
                            result = pc.write_rtcp(&[Box::new(PictureLossIndication{
                                sender_ssrc: 0,
                                media_ssrc,
                            })]).await.map_err(Into::into);
                        }else{
                            break;
                        }
                    }
                };
            }
        });

        let local_track_chan_tx2 = Arc::clone(&local_track_chan_tx);
        tokio::spawn(async move {
            // Create Track that we send video back to browser on
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                "video".to_owned(),
                "webrtc-rs".to_owned(),
            ));
            let _ = local_track_chan_tx2.send(Arc::clone(&local_track)).await;

            // Read RTP packets being sent to webrtc-rs
            while let Ok((rtp, _)) = track.read_rtp().await {
                if let Err(err) = local_track.write_rtp(&rtp).await {
                    if Error::ErrClosedPipe != err {
                        print!("output track write_rtp got error: {err} and break");
                        break;
                    } else {
                        print!("output track write_rtp got error: {err}");
                    }
                }
            }
        });

        Box::pin(async {})
    }));

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

    if let Some(local_track) = local_track_chan_rx.recv().await {
        loop {
            println!("\nCurl an base64 SDP to start sendonly peer connection");

            let recv_only_offer = signaling.wait_for_offer().await?;

            // Create a MediaEngine object to configure the supported codec
            let mut m = MediaEngine::default();

            m.register_default_codecs()?;

            // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
            // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
            // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
            // for each PeerConnection.
            let mut registry = Registry::new();

            // Use the default set of Interceptors
            registry = register_default_interceptors(registry, &mut m)?;

            // Create the API object with the MediaEngine
            let api = APIBuilder::new()
                .with_media_engine(m)
                .with_interceptor_registry(registry)
                .build();

            // Prepare the configuration
            let config = RTCConfiguration {
                ice_servers: vec![RTCIceServer {
                    urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                    ..Default::default()
                }],
                ..Default::default()
            };

            // Create a new RTCPeerConnection
            let peer_connection = Arc::new(api.new_peer_connection(config).await?);

            let rtp_sender = peer_connection
                .add_track(Arc::clone(&local_track) as Arc<dyn TrackLocal + Send + Sync>)
                .await?;

            // Read incoming RTCP packets
            // Before these packets are returned they are processed by interceptors. For things
            // like NACK this needs to be called.
            tokio::spawn(async move {
                let mut rtcp_buf = vec![0u8; 1500];
                while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                Result::<()>::Ok(())
            });

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