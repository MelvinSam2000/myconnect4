use std::collections::HashMap;
use std::sync::Arc;

use either::Either;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

use super::bot;
use super::service;
use super::BUFFER_MAX;
use crate::actor::bot::BotActor;

#[derive(Debug)]
pub enum MessageIn {
    HeartBeat { respond_to: oneshot::Sender<()> },
    QueueOne,
    SpawnSeveral { number: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageOut {
    Nothing,
}

pub struct BotManagerActor {
    tx_in: Sender<MessageIn>,
    #[allow(dead_code)]
    tx_out: Sender<MessageOut>,
    rx_in: Receiver<MessageIn>,
    tx_s_in: Sender<service::MessageIn>,
    tx_s_out: Sender<service::MessageOut>,
    rx_s_in: Receiver<service::MessageIn>,
    tx_b_out: Sender<bot::MessageOut>,
    rx_b_out: Receiver<bot::MessageOut>,
    map_bots:
        Arc<RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>>,
}

impl BotManagerActor {
    pub fn new(tx_bm_out: Sender<MessageOut>, tx_s_out_bot: Sender<service::MessageOut>) -> Self {
        let (tx_in, rx_in) = mpsc::channel(BUFFER_MAX);
        let (tx_s_in, rx_s_in) = mpsc::channel(BUFFER_MAX);
        let (tx_b_out, rx_b_out) = mpsc::channel(BUFFER_MAX);
        Self {
            tx_in,
            tx_out: tx_bm_out,
            rx_in,
            tx_s_in,
            tx_s_out: tx_s_out_bot,
            rx_s_in,
            tx_b_out,
            rx_b_out,
            map_bots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn get_sender(&self) -> Sender<MessageIn> {
        self.tx_in.clone()
    }

    pub fn get_service_sender(&self) -> Sender<service::MessageIn> {
        self.tx_s_in.clone()
    }

    async fn send_to_bot(
        map_bots: &Arc<
            RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>,
        >,
        bot_id: &str,
        msg: Either<bot::MessageIn, service::MessageInInner>,
    ) {
        let map_bots = map_bots.read().await;
        let Some((bsender, ssender)) = map_bots.get(bot_id) else {
            return;
        };

        match msg {
            Either::Left(msg) => bsender.send(msg).await.unwrap(),
            Either::Right(msg) => ssender.send(msg).await.unwrap(),
        }
    }

    pub fn start(self) {
        let BotManagerActor {
            tx_in: _,
            tx_out: _,
            mut rx_in,
            tx_s_in: _,
            tx_s_out,
            mut rx_s_in,
            tx_b_out,
            mut rx_b_out,
            map_bots,
        } = self;

        log::debug!("BotManager actor started.");
        // main event listening
        let map_bots_1 = map_bots.clone();
        tokio::spawn(async move {
            let map_bots = map_bots_1;
            'msg: while let Some(msg) = rx_in.recv().await {
                log::debug!("RECV {msg:?}");
                match msg {
                    MessageIn::HeartBeat { respond_to } => {
                        let _ = respond_to.send(());
                    }
                    MessageIn::QueueOne => {
                        // TODO, probe for bot idle before spawning
                        let rmap_bots = map_bots.read().await;

                        let mut tasks = JoinSet::new();
                        rmap_bots
                            .values()
                            .map(|(sender, _)| {
                                let (tx, rx) = oneshot::channel();
                                let sender = sender.clone();
                                async move {
                                    sender
                                        .send(bot::MessageIn::QueryIdle { respond_to: tx })
                                        .await
                                        .unwrap();
                                    let is_idle = rx.await.unwrap();
                                    (is_idle, sender)
                                }
                            })
                            .for_each(|task| {
                                tasks.spawn(task);
                            });

                        while let Some(Ok((is_idle, sender))) = tasks.join_next().await {
                            if is_idle {
                                sender.send(bot::MessageIn::SearchForGame).await.unwrap();
                                continue 'msg;
                            }
                        }

                        drop(rmap_bots);

                        let bot_id = Self::spawn_bot(&map_bots, &tx_b_out, &tx_s_out).await;
                        //tx_out.send(MessageOut::Nothing).await.unwrap();
                        Self::send_to_bot(
                            &map_bots,
                            &bot_id,
                            Either::Left(bot::MessageIn::SearchForGame),
                        )
                        .await;
                    }
                    MessageIn::SpawnSeveral { number } => {
                        let map_bots = map_bots.clone();
                        let mut tasks = JoinSet::new();
                        (0..number)
                            .map(|_| (map_bots.clone(), tx_b_out.clone(), tx_s_out.clone()))
                            .map(|(map_bots, tx_b_out, tx_s_out)| async move {
                                let bot_id = Self::spawn_bot(&map_bots, &tx_b_out, &tx_s_out).await;
                                Self::send_to_bot(
                                    &map_bots,
                                    &bot_id,
                                    Either::Left(bot::MessageIn::PermaPlay(true)),
                                )
                                .await;
                                Self::send_to_bot(
                                    &map_bots,
                                    &bot_id,
                                    Either::Left(bot::MessageIn::SearchForGame),
                                )
                                .await;
                            })
                            .for_each(|task| {
                                tasks.spawn(task);
                            });
                        for _ in 0..number {}
                        log::debug!("Spawning bots...");
                        while let Some(_) = tasks.join_next().await {}
                        log::debug!("Done spawning bots");
                    }
                }
            }
        });

        // listening for player events
        let map_bots_2 = map_bots.clone();
        tokio::spawn(async move {
            let map_bots = map_bots_2;
            while let Some(msg) = rx_s_in.recv().await {
                log::debug!("RECV {msg:?}");
                let service::MessageIn { user, inner } = msg;
                Self::send_to_bot(&map_bots, &user, Either::Right(inner)).await;
            }
        });

        // listening for bot events
        tokio::spawn(async move {
            while let Some(msg) = rx_b_out.recv().await {
                match msg {
                    bot::MessageOut::Nothing => {
                        log::debug!("RECV {msg:?}");
                        log::warn!("Ignoring for now");
                    }
                }
            }
        });
    }

    async fn spawn_bot(
        map_bots: &Arc<
            RwLock<HashMap<String, (Sender<bot::MessageIn>, Sender<service::MessageInInner>)>>,
        >,
        tx_b_out: &Sender<bot::MessageOut>,
        tx_s_out: &Sender<service::MessageOut>,
    ) -> String {
        let tx_out = tx_b_out.clone();
        let tx_s_out = tx_s_out.clone();
        let bot = BotActor::new(tx_out, tx_s_out);

        let bot_id = bot.get_id();
        let bsender = bot.get_sender();
        let ssender = bot.get_service_sender();

        map_bots
            .write()
            .await
            .insert(bot_id.clone(), (bsender, ssender));

        bot.start();

        bot_id
    }
}
