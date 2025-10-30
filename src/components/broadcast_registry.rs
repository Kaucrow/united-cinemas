use crate::prelude::*;
use anyhow::Result;

pub struct Broadcast {
    pub name: String,
    pub track: Arc<TrackLocalStaticRTP>,
    pub peer_connection: Arc<RTCPeerConnection>,
}

pub type BroadcastRegistry = Arc<Mutex<HashMap<String, Broadcast>>>;

pub struct BroadcastManager {
    registry: BroadcastRegistry,
}

impl BroadcastManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn get_registry(&self) -> BroadcastRegistry {
        Arc::clone(&self.registry)
    }
    pub async fn register_broadcast(&self, name: String, track: Arc<TrackLocalStaticRTP>, peer_connection: Arc<RTCPeerConnection>) {
        let mut registry = self.registry.lock().await;
        info!("Registering broadcast: {}", name);
        registry.insert(name.clone(), Broadcast { name, track, peer_connection });
    }
    pub async fn unregister_broadcast(&self, name: &str) {
        let mut registry = self.registry.lock().await;
        if registry.remove(name).is_some() {
            info!("Unregistered broadcast: {}", name);
        } else {
            warn!("Attempted to unregister non-existent broadcast: {}", name);
        }
    }
    pub async fn get_broadcast(&self, name: &str) -> Option<Arc<TrackLocalStaticRTP>> {
        let registry = self.registry.lock().await;
        registry.get(name).map(|b| Arc::clone(&b.track))
    }

}