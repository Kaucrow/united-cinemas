use crate::{
    components::{ PeerConnectionFactory, TrackManager },
    prelude::*
};
use anyhow::Result;

pub struct SessionManager {
    peer_conn_factory: Arc<PeerConnectionFactory>,
}

impl SessionManager {
    pub fn new(peer_conn_factory: Arc<PeerConnectionFactory>) -> Self {
        Self { peer_conn_factory }
    }

    pub async fn create_broadcaster_session(
        &self,
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

        // Setup track handlers
        let _ = track_manager.setup_track_handlers(Arc::clone(&peer_connection))?;

        // Setup connection state handler
        self.setup_conn_state_handler(Arc::clone(&peer_connection));

        // Handle offer
        peer_connection.set_remote_description(offer).await?;

        Ok(peer_connection)
    }

    pub async fn create_viewer_session(
        &self,
        offer: RTCSessionDescription,
        local_track: Arc<TrackLocalStaticRTP>
    ) -> Result<Arc<RTCPeerConnection>> {
        let peer_connection = self.peer_conn_factory
            .create_recv_only_peer_connection(local_track)
            .await?;

        // Setup connection state handler
        self.setup_conn_state_handler(Arc::clone(&peer_connection));

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

    fn setup_conn_state_handler(&self, peer_connection: Arc<RTCPeerConnection>) {
        peer_connection.on_peer_connection_state_change(Box::new(
            move |s: RTCPeerConnectionState| {
                debug!("Peer connection state has changed: {s}");
                Box::pin(async {})
            }
        ));
    }
}