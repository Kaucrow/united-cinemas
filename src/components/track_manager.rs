use std::time::Duration;

use crate::prelude::*;
use anyhow::Result;

pub struct TrackManager {
    broadcast: String,
    video_track_chan_tx: Arc<mpsc::Sender<Arc<TrackLocalStaticRTP>>>,
    video_track_chan_rx: mpsc::Receiver<Arc<TrackLocalStaticRTP>>,
    audio_track_chan_tx: Arc<mpsc::Sender<Arc<TrackLocalStaticRTP>>>,
    audio_track_chan_rx: mpsc::Receiver<Arc<TrackLocalStaticRTP>>,
}

impl TrackManager {
    pub fn new(broadcast: String) -> Self {
        let (video_track_chan_tx, video_track_chan_rx) =
            mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);
        let (audio_track_chan_tx, audio_track_chan_rx) =
            mpsc::channel::<Arc<TrackLocalStaticRTP>>(1);

        Self {
            broadcast,
            video_track_chan_tx: Arc::new(video_track_chan_tx),
            video_track_chan_rx,
            audio_track_chan_tx: Arc::new(audio_track_chan_tx),
            audio_track_chan_rx,
        }
    }

    pub fn get_video_track_receiver(&mut self) -> &mut mpsc::Receiver<Arc<TrackLocalStaticRTP>> {
        &mut self.video_track_chan_rx
    }

    pub fn get_audio_track_receiver(&mut self) -> &mut mpsc::Receiver<Arc<TrackLocalStaticRTP>> {
        &mut self.audio_track_chan_rx
    }

    pub fn setup_track_handlers(
        &self,
        peer_connection: Arc<RTCPeerConnection>
    ) -> Result<()> {
        let video_track_sender = Arc::clone(&self.video_track_chan_tx);
        let audio_track_sender = Arc::clone(&self.audio_track_chan_tx);
        let peer_conn_weak = Arc::downgrade(&peer_connection);
        let broadcast = self.broadcast.clone();

        peer_connection.on_track(Box::new(move |track, _, _| {
            let video_track_sender = Arc::clone(&video_track_sender);
            let audio_track_sender = Arc::clone(&audio_track_sender);
            let peer_conn_weak = peer_conn_weak.clone();
            let broadcast = broadcast.clone();

            debug!("Broadcast '{}': Received {} track (SSRC: {})", 
                   broadcast, track.kind(), track.ssrc());

            match track.kind() {
                RTPCodecType::Video => {
                    // Spawn PLI (Picture Loss Indication) sender for video
                    Self::spawn_pli_sender(broadcast.clone(), peer_conn_weak.clone(), track.ssrc());
                    
                    // Spawn video track relay
                    Self::spawn_track_relay(broadcast.clone(), "video", track, video_track_sender);
                }
                RTPCodecType::Audio => {
                    // Spawn audio track relay (no PLI needed for audio)
                    Self::spawn_track_relay(broadcast.clone(), "audio", track, audio_track_sender);
                }
                RTPCodecType::Unspecified => {
                    error!("Broadcast '{}': Got unspecified track type", broadcast);
                }
            }

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
            debug!("Broadcast '{}': Starting PLI sender for video (SSRC: {})", broadcast, media_ssrc);
            
            while result.is_ok() {
                tokio::time::sleep(Duration::from_secs(3)).await;

                if let Some(peer_connection) = peer_conn_weak.upgrade() {
                    debug!("Broadcast '{}': Sending PLI for video (SSRC: {})", broadcast, media_ssrc);
                    
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
        track_type: &'static str,
        track: Arc<TrackRemote>,
        track_sender: Arc<mpsc::Sender<Arc<TrackLocalStaticRTP>>>
    ) {
        tokio::spawn(async move {
            let local_track = Arc::new(TrackLocalStaticRTP::new(
                track.codec().capability,
                track_type.to_owned(),
                "webrtc-rs".to_owned(),
            ));

            let _ = track_sender.send(Arc::clone(&local_track)).await;

            debug!("Broadcast '{}': {} track relay started, waiting for RTP packets...", 
                   broadcast, track_type);

            let mut packet_count = 0;
            while let Ok((rtp, _)) = track.read_rtp().await {
                packet_count += 1;
                if packet_count % 100 == 0 {
                    debug!("Broadcast '{}': Relayed {} {} RTP packets", 
                           broadcast, packet_count, track_type);
                }

                if let Err(err) = local_track.write_rtp(&rtp).await {
                    if Error::ErrClosedPipe != err {
                        debug!("Broadcast '{}': {} track relay error: {}, stopping", 
                               broadcast, track_type, err);
                        break;
                    } else {
                        debug!("Broadcast '{}': {} track relay closed pipe: {}", 
                               broadcast, track_type, err);
                    }
                }
            }
            debug!("Broadcast '{}': {} track relay ended", broadcast, track_type);
        });
    }
}