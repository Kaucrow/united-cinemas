use crate::{
    components::{ PeerConnectionFactory, TrackManager, BroadcastManager },
    prelude::*
};
use anyhow::Result;

#[derive(Clone)]
pub struct SessionManager {
    peer_conn_factory: Arc<PeerConnectionFactory>,
    broadcast_manager: Arc<BroadcastManager>
}

impl SessionManager {
    pub fn new(peer_conn_factory: Arc<PeerConnectionFactory>, broadcast_manager: Arc<BroadcastManager>) -> Self {
        Self { peer_conn_factory, broadcast_manager }
    }

    pub async fn create_broadcaster_session(
        &self,
        broadcast: String,
        offer: RTCSessionDescription,
        track_manager: &mut TrackManager
    ) -> Result<Arc<RTCPeerConnection>> {
        let peer_connection = self.peer_conn_factory
            .create_peer_connection()
            .await?;

        // Add transceiver for receiving video
        peer_connection
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await?;

        peer_connection
            .add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await?;

        // Setup track handlers
        let _ = track_manager.setup_track_handlers(Arc::clone(&peer_connection))?;

        // Setup connection state handler
        self.setup_conn_state_handler(
            broadcast,
            true,
            Arc::clone(&peer_connection),
            Arc::clone(&self.broadcast_manager)
        ).await;

        // Handle offer
        peer_connection.set_remote_description(offer).await?;

        Ok(peer_connection)
    }

    pub async fn create_viewer_session(
        &self,
        broadcast: String,
        offer: RTCSessionDescription,
        video_track: Arc<TrackLocalStaticRTP>,
        audio_track: Arc<TrackLocalStaticRTP>
    ) -> Result<Arc<RTCPeerConnection>> {
        let peer_connection = self.peer_conn_factory
            .create_recv_only_peer_connection(video_track, audio_track)
            .await?;

        // Setup connection state handler
        self.setup_conn_state_handler(
            broadcast,
            false,
            Arc::clone(&peer_connection),
            Arc::clone(&self.broadcast_manager)
        ).await;

        // Handle offer
        peer_connection.set_remote_description(offer).await?;

        Ok(peer_connection)
    }

    pub async fn create_answer(
        &self,
        peer_connection: &Arc<RTCPeerConnection>
    ) -> Result<RTCSessionDescription> {
        let answer = peer_connection.create_answer(None).await?;

        let mut gather_complete = peer_connection.gathering_complete_promise().await;

        peer_connection.set_local_description(answer).await?;

        // Block until ICE Gathering is complete, disabling trickle ICE.
        // We do this because we only can exchange one signaling message
        // in a production application we should exchange ICE Candidates via OnICECandidate.
        let _ = gather_complete.recv().await;

        peer_connection.local_description().await
            .ok_or_else(|| anyhow::anyhow!("Failed to get local description"))
    }

    async fn setup_conn_state_handler(
        &self,
        broadcast: String,
        is_broadcaster: bool,
        peer_connection: Arc<RTCPeerConnection>,
        broadcast_manager: Arc<BroadcastManager>
    ) {
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                debug!("Broadcast '{}': Peer connection state has changed: {s}", &broadcast);

                if is_broadcaster {
                    match s {
                        RTCPeerConnectionState::Closed => {
                            let broadcast_manager = Arc::clone(&broadcast_manager);
                            let broadcast = broadcast.clone();

                            tokio::spawn(async move {
                                debug!("Broadcast '{}': Broadcaster disconnected, unregistering", &broadcast);
                                broadcast_manager.unregister_broadcast(&broadcast).await;
                            });
                        }
                        _ => {}
                    }
                }

                Box::pin(async {})
            }
        ));
    }
}