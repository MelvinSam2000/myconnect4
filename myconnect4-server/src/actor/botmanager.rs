use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

use super::service;
use super::BUFFER_MAX;

#[derive(Debug)]
pub enum MessageIn {
    QueueOne,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    Nothing,
}

pub struct BotManagerActor {
    tx_in: Sender<MessageIn>,
    tx_out: Sender<MessageOut>,
    rx_in: Receiver<MessageIn>,
    tx_s_in: Sender<service::MessageIn>,
    tx_s_out: Sender<service::MessageOut>,
    rx_s_in: Receiver<service::MessageIn>,
}

impl BotManagerActor {
    pub fn new(tx_bm_out: Sender<MessageOut>, tx_s_out_bot: Sender<service::MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let (tx_s_in, rx_s_in) = mpsc::channel(BUFFER_MAX);
        Self {
            tx_in,
            tx_out: tx_bm_out,
            rx_in,
            tx_s_in,
            tx_s_out: tx_s_out_bot,
            rx_s_in,
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn get_service_sender(&self) -> Sender<service::MessageIn> {
        self.tx_s_in.clone()
    }

    pub fn start(mut self) {
        log::debug!("BotManager actor started.");
        // main event listening
        tokio::spawn(async move {
            while let Some(req) = self.rx_in.recv().await {
                log::debug!("RECV {req:?}");
                match req {
                    MessageIn::QueueOne => {
                        log::warn!("Ignoring message");
                    }
                }
            }
        });

        // listening for player events
        tokio::spawn(async move {
            while let Some(req) = self.rx_s_in.recv().await {
                log::debug!("RECV {req:?}");
                log::warn!("Ignoring for now");
            }
        });
    }
}
