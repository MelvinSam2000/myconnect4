pub enum MessageFromQueueing {
    GameSearch { user: String },
    GameMove { user: String, col: u8 },
}

pub enum MessageFromGame {}

/*
struct MainControllerActor {
    tx_from_queueing: Sender<MessageFromQueueing>,
    rx_from_queueing: Receiver<MessageFromQueueing>,
    connectors: HashMap<String, Sender<connector::Message>>,
}

impl MainControllerActor {
    fn new(
        tx_to_queueing: Sender<>
    ) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_MAX_CAPACITY);
        let connectors = HashMap::new();
        Self { tx, rx, connectors }
    }

    fn get_sender(&self) -> Sender<Message> {
        self.tx.clone()
    }

    fn start(mut self) {
        tokio::spawn(async move {
            while let Some(message) = self.rx.recv().await {
                match message {
                    Message::GameSearch { user } => {
                        if let Some(tx) = self.connectors.get(&user) {
                            tx.send(connector::Message::GameSearch { user }).await;
                        }
                    }
                    Message::GameMove { user, col } => todo!(),
                }
            }
        });
    }
}
*/
