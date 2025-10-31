use crate::prelude::*;
use anyhow::Result;

pub struct PeerConnectionFactory {
    api: webrtc::api::API,
}

impl PeerConnectionFactory {
    pub async fn new() -> Result<Self> {
        let mut media_eng = MediaEngine::default();
        media_eng.register_default_codecs()?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_eng)?;

        let api = APIBuilder::new()
            .with_media_engine(media_eng)
            .with_interceptor_registry(registry)
            .build();

        Ok(Self { api })
    }

    pub async fn create_peer_connection(&self) -> Result<Arc<RTCPeerConnection>> {
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        Ok(Arc::new(self.api.new_peer_connection(config).await?))
    }

    pub async fn create_recv_only_peer_connection(
        &self,
        video_track: Arc<TrackLocalStaticRTP>,
        audio_track: Arc<TrackLocalStaticRTP>,
    ) -> Result<Arc<RTCPeerConnection>> {
        let peer_connection = self.create_peer_connection().await?;

        let video_sender = peer_connection
            .add_track(video_track as Arc<dyn TrackLocal + Send + Sync>)
            .await?;
        
        let audio_sender = peer_connection
            .add_track(audio_track as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // Handle RTCP packets
        self.spawn_rtcp_handler(video_sender);
        self.spawn_rtcp_handler(audio_sender);

        Ok(peer_connection)
    }

    // Read incoming RTCP packets
    // Before these packets are returned they are processed by interceptors. For things
    // like NACK this needs to be called.
    fn spawn_rtcp_handler(&self, rtp_sender: Arc<RTCRtpSender>) {
        tokio::spawn(async move {
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
            Result::<()>::Ok(())
        });
    }
}