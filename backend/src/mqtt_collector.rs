use crate::config::MqttConfig;
use crate::models::{NUM_CABINETS};
use anyhow::Result;
use chrono::DateTime;
use dashmap::DashMap;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

#[derive(Debug, Deserialize, Clone)]
pub struct RawChannelData {
    pub timestamp: String,
    pub cabinet_id: u16,
    pub channel_id: u32,
    pub voltage: f64,
    pub current: f64,
    pub temperature: f64,
    pub capacity: f64,
    pub cycle_index: u16,
    pub stage: String,
    pub stage_duration: u32,
}

pub type DataSender = mpsc::Sender<Vec<RawChannelData>>;
pub type DataReceiver = mpsc::Receiver<Vec<RawChannelData>>;

type CabinetSender = mpsc::UnboundedSender<Vec<RawChannelData>>;

pub struct MqttCollector {
    config: MqttConfig,
    client: AsyncClient,
    eventloop: Arc<Mutex<Option<EventLoop>>>,
    cabinet_senders: DashMap<u16, CabinetSender>,
    cabinet_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    data_sender: DataSender,
}

impl MqttCollector {
    pub fn new(config: MqttConfig, data_sender: DataSender) -> Result<Self> {
        let mut options = MqttOptions::new(&config.client_id, &config.broker, config.port);
        options.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(options, 2048);

        Ok(Self {
            config,
            client,
            eventloop: Arc::new(Mutex::new(Some(eventloop))),
            cabinet_senders: DashMap::new(),
            cabinet_tasks: Arc::new(Mutex::new(Vec::new())),
            data_sender,
        })
    }

    pub async fn start(&self) -> Result<()> {
        self.spawn_cabinet_routers().await;
        info!("Spawned {} cabinet routers", NUM_CABINETS);

        let subscribe_topic = self.config.subscribe_topic.clone();
        self.client
            .subscribe(&subscribe_topic, QoS::AtLeastOnce)
            .await?;
        info!("Subscribed to MQTT topic: {}", subscribe_topic);

        let eventloop = self.eventloop.lock().await.take().unwrap();
        let client = self.client.clone();
        let cabinet_senders = self.cabinet_senders.clone();
        let data_sender = self.data_sender.clone();

        tokio::spawn(async move {
            Self::run_event_loop(eventloop, client, cabinet_senders, data_sender).await;
        });

        Ok(())
    }

    async fn spawn_cabinet_routers(&self) {
        let mut tasks = self.cabinet_tasks.lock().await;

        for cabinet_id in 0..NUM_CABINETS as u16 {
            let (tx, mut rx) = mpsc::unbounded_channel::<Vec<RawChannelData>>();
            let data_sender = self.data_sender.clone();

            let handle = tokio::spawn(async move {
                info!("Cabinet {} router started", cabinet_id);

                while let Some(batch) = rx.recv().await {
                    if let Err(e) = data_sender.send(batch).await {
                        warn!(
                            "Cabinet {} router failed to send data: {}",
                            cabinet_id, e
                        );
                    }
                }

                warn!("Cabinet {} router stopped", cabinet_id);
            });

            self.cabinet_senders.insert(cabinet_id, tx);
            tasks.push(handle);
        }
    }

    async fn run_event_loop(
        mut eventloop: EventLoop,
        _client: AsyncClient,
        cabinet_senders: DashMap<u16, CabinetSender>,
        _data_sender: DataSender,
    ) {
        info!("MQTT collector event loop started");

        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    if let Err(e) =
                        Self::route_message(publish.payload.as_ref(), &cabinet_senders)
                    {
                        warn!("Failed to route MQTT message: {}", e);
                    }
                }
                Ok(Event::Outgoing(_)) => {}
                Ok(_) => {}
                Err(e) => {
                    error!("MQTT eventloop error: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    fn route_message(
        payload: &[u8],
        cabinet_senders: &DashMap<u16, CabinetSender>,
    ) -> Result<()> {
        let raw_data: Vec<RawChannelData> = serde_json::from_slice(payload)?;

        if raw_data.is_empty() {
            return Ok(());
        }

        let cabinet_id = raw_data[0].cabinet_id;

        if let Some(sender) = cabinet_senders.get(&cabinet_id) {
            if let Err(e) = sender.send(raw_data) {
                warn!(
                    "Failed to send batch to cabinet {} router: {}",
                    cabinet_id, e
                );
            }
        } else {
            warn!(
                "No router found for cabinet {}, message dropped",
                cabinet_id
            );
        }

        Ok(())
    }

    pub fn parse_raw_data(&self, raw: &RawChannelData) -> Result<(chrono::DateTime<chrono::Utc>, crate::models::Stage)> {
        let timestamp = DateTime::parse_from_rfc3339(&raw.timestamp)?;
        let timestamp = timestamp.with_timezone(&chrono::Utc);
        let stage = crate::models::Stage::from_str(&raw.stage).unwrap_or(crate::models::Stage::Rest);
        Ok((timestamp, stage))
    }
}
