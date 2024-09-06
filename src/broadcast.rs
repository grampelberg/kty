use std::{collections::HashMap, sync::Arc};

use eyre::{eyre, Result};
use russh::ChannelId;
use tokio::sync::{mpsc::UnboundedSender, Mutex};

use crate::events::Event;

#[derive(Debug, Clone, Default)]
pub struct Broadcast {
    channels: Arc<Mutex<HashMap<ChannelId, UnboundedSender<Event>>>>,
}

impl Broadcast {
    pub async fn add(&mut self, id: ChannelId, tx: UnboundedSender<Event>) -> Result<()> {
        self.channels.lock().await.insert(id, tx);

        Ok(())
    }

    pub async fn remove(&mut self, id: &ChannelId) -> Option<UnboundedSender<Event>> {
        self.channels.lock().await.remove(id)
    }

    pub async fn send(&self, id: &ChannelId, event: Event) -> Result<()> {
        let mut channels = self.channels.lock().await;
        if let Some(sender) = channels.get_mut(id) {
            sender
                .send(event)
                .map_err(|_| eyre!("failed to send event"))?;
        }
        Ok(())
    }

    pub async fn all(&self, event: Event) -> Result<()> {
        let mut channels = self.channels.lock().await;
        for sender in channels.values_mut() {
            sender.send(event.clone())?;
        }

        Ok(())
    }
}
