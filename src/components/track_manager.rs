use std::time::Duration;

use crate::prelude::*;
use anyhow::Result;

pub struct TrackManager {
    // uwu
    broadcast: String,
    local_track_chan_tx: Arc<mpsc::Sender<Arc<TrackLocalStaticRTP>>>,
    local_track_chan_rx: mpsc::Receiver<Arc<TrackLocalStaticRTP>>,
}

impl TrackManager {
    pub fn new(broadcast: String) -> Self {
        let (local_track_chan_tx, local_track_chan_rx) =
            mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);

        Self {
            broadcast,
            local_track_chan_tx: Arc::new(local_track_chan_tx),
            local_track_chan_rx,
        }
    }

    pub fn get_track_receiver(&mut self) -> &mut mpsc::Receiver<Arc<TrackLocalStaticRTP>> {
        &mut self.local_track_chan_rx
    }

    pub fn setup_track_handlers(
        &self,
        peer_connection: Arc<RTCPeerConnection>
    ) -> Result<()> {
        let track_sender = Arc::clone(&self.local_track_chan_tx);
        let peer_conn_weak = Arc::downgrade(&peer_connection);
        let broadcast = self.broadcast.clone();

        peer_connection.on_track(Box::new(move |track, _, _| {
            let track_sender = Arc::clone(&track_sender);
            let peer_conn_weak = peer_conn_weak.clone();
            let broadcast = broadcast.clone();

            // Spawn PLI (Picture Loss Indication) sender
            Self::spawn_pli_sender(broadcast.clone(), peer_conn_weak.clone(), track.ssrc());

            // Spawn track relay
            Self::spawn_track_relay(broadcast, track, track_sender);

            Box::pin(async {})
        }));

        Ok(())
    }

    fn spawn_pli_sender(
        broadcast: String,
        peer_conn_weak: Weak<RTCPeerConnection>,
        media_ssrc: u32
    ) {
        tokio::spawn(async move {
            let mut result = Result::<usize>::Ok(0);
            debug!("Broadcast '{}': Starting PLI sender (SSRC: {})", broadcast, media_ssrc);
            
            while result.is_ok() {
                tokio::time::sleep(Duration::from_secs(3)).await;

                if let Some(peer_connection) = peer_conn_weak.upgrade() {
                    debug!("Broadcast '{}': Sending PLI (SSRC: {})", broadcast, media_ssrc);
                    
                    result = peer_connection.write_rtcp(&[Box::new(PictureLossIndication {
                        sender_ssrc: 0,
                        media_ssrc,
                    })]).await.map_err(Into::into);
 
                    if let Err(e) = &result {
                        debug!("Broadcast '{}': PLI send failed: {}", broadcast, e);
                    } else {
                        debug!("Broadcast '{}': PLI sent successfully", broadcast);
                    }
                } else {
                    debug!("Broadcast '{}': Peer connection closed, stopping PLI sender", broadcast);
                    break;
                }
            }
            debug!("Broadcast '{}': PLI sender terminated", broadcast);
        });
    }

    fn spawn_track_relay(
        broadcast: String,
        track: Arc<TrackRemote>,
        track_sender: Arc<mpsc::Sender<Arc<TrackLocalStaticRTP>>>
    ) {
        tokio::spawn(async move {
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                "video".to_owned(),
                "webrtc-rs".to_owned(),
            ));

            let _ = track_sender.send(Arc::clone(&local_track)).await;

            debug!("Broadcast '{}': Track relay started, waiting for RTP packets...", broadcast);

            let mut packet_count = 0;
            while let Ok((rtp, _)) = track.read_rtp().await {
                packet_count += 1;
                if packet_count % 100 == 0 {
                    debug!("Broadcast '{}': Relayed {} RTP packets", broadcast, packet_count);
                }

                if let Err(err) = local_track.write_rtp(&rtp).await {
                    if Error::ErrClosedPipe != err {
                        debug!("Broadcast '{}': Track relay error: {}, stopping", broadcast, err);
                        break;
                    } else {
                        debug!("Broadcast '{}': Track relay closed pipe: {}", broadcast, err);
                    }
                }
            }
            debug!("Broadcast '{}': Track relay ended unexpectedly", broadcast);
        });
    }
}