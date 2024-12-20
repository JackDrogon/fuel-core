use fuel_core_services::{
    RunnableService,
    RunnableTask,
    ServiceRunner,
    StateWatcher,
    TaskNextAction,
};
use std::{
    sync::{
        Arc,
        Mutex,
    },
    time::Duration,
};
use tokio::{
    sync::broadcast::Sender,
    time::{
        interval,
        Interval,
    },
};

use crate::v1::da_source_service::DaBlockCosts;
pub use anyhow::Result;
use fuel_core_types::fuel_types::BlockHeight;

#[derive(Clone)]
pub struct SharedState(Sender<DaBlockCosts>);

impl SharedState {
    fn new(sender: Sender<DaBlockCosts>) -> Self {
        Self(sender)
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaBlockCosts> {
        self.0.subscribe()
    }
}

/// This struct houses the shared_state, polling interval
/// and a source, which does the actual fetching of the data
pub struct DaSourceService<Source> {
    poll_interval: Interval,
    source: Source,
    shared_state: SharedState,
    latest_l2_height: Arc<Mutex<BlockHeight>>,
    recorded_height: Option<BlockHeight>,
}

pub(crate) const DA_BLOCK_COSTS_CHANNEL_SIZE: usize = 16 * 1024;
const POLLING_INTERVAL_MS: u64 = 10_000;

impl<Source> DaSourceService<Source>
where
    Source: DaBlockCostsSource,
{
    pub fn new(
        source: Source,
        poll_interval: Option<Duration>,
        latest_l2_height: Arc<Mutex<BlockHeight>>,
        recorded_height: Option<BlockHeight>,
    ) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(DA_BLOCK_COSTS_CHANNEL_SIZE);
        #[allow(clippy::arithmetic_side_effects)]
        Self {
            shared_state: SharedState::new(sender),
            poll_interval: interval(
                poll_interval.unwrap_or(Duration::from_millis(POLLING_INTERVAL_MS)),
            ),
            source,
            latest_l2_height,
            recorded_height,
        }
    }

    #[cfg(test)]
    pub fn new_with_sender(
        source: Source,
        poll_interval: Option<Duration>,
        latest_l2_height: Arc<Mutex<BlockHeight>>,
        recorded_height: Option<BlockHeight>,
        sender: Sender<DaBlockCosts>,
    ) -> Self {
        Self {
            shared_state: SharedState::new(sender),
            poll_interval: interval(
                poll_interval.unwrap_or(Duration::from_millis(POLLING_INTERVAL_MS)),
            ),
            source,
            latest_l2_height,
            recorded_height,
        }
    }

    async fn process_block_costs(&mut self) -> Result<()> {
        let da_block_costs_res = self
            .source
            .request_da_block_costs(&self.recorded_height)
            .await;
        tracing::debug!("Received block costs: {:?}", da_block_costs_res);
        let da_block_costs = da_block_costs_res?;
        let filtered_block_costs = self
            .filter_costs_that_have_values_greater_than_l2_block_height(da_block_costs)?;
        tracing::debug!(
            "the latest l2 height is: {:?}",
            *self.latest_l2_height.lock().unwrap()
        );
        for da_block_costs in filtered_block_costs {
            tracing::debug!("Sending block costs: {:?}", da_block_costs);
            let end = BlockHeight::from(*da_block_costs.l2_blocks.end());
            self.shared_state.0.send(da_block_costs)?;
            if let Some(recorded_height) = self.recorded_height {
                if end > recorded_height {
                    self.recorded_height = Some(end)
                }
            } else {
                self.recorded_height = Some(end)
            }
        }
        Ok(())
    }

    fn filter_costs_that_have_values_greater_than_l2_block_height(
        &self,
        da_block_costs: Vec<DaBlockCosts>,
    ) -> Result<impl Iterator<Item = DaBlockCosts>> {
        let latest_l2_height = *self
            .latest_l2_height
            .lock()
            .map_err(|err| anyhow::anyhow!("lock error: {:?}", err))?;
        let iter = da_block_costs.into_iter().filter(move |da_block_costs| {
            let end = BlockHeight::from(*da_block_costs.l2_blocks.end());
            end < latest_l2_height
        });
        Ok(iter)
    }

    #[cfg(test)]
    pub fn recorded_height(&self) -> Option<BlockHeight> {
        self.recorded_height
    }
}

/// This trait is implemented by the sources to obtain the
/// da block costs in a way they see fit
#[async_trait::async_trait]
pub trait DaBlockCostsSource: Send + Sync {
    async fn request_da_block_costs(
        &mut self,
        recorded_height: &Option<BlockHeight>,
    ) -> Result<Vec<DaBlockCosts>>;
}

#[async_trait::async_trait]
impl<Source> RunnableService for DaSourceService<Source>
where
    Source: DaBlockCostsSource,
{
    const NAME: &'static str = "DaSourceService";

    type SharedData = SharedState;

    type Task = Self;

    type TaskParams = ();

    fn shared_data(&self) -> Self::SharedData {
        self.shared_state.clone()
    }

    async fn into_task(
        mut self,
        _: &StateWatcher,
        _: Self::TaskParams,
    ) -> Result<Self::Task> {
        self.poll_interval.reset();
        Ok(self)
    }
}

#[async_trait::async_trait]
impl<Source> RunnableTask for DaSourceService<Source>
where
    Source: DaBlockCostsSource,
{
    /// This function polls the source according to a polling interval
    /// described by the DaBlockCostsService
    async fn run(&mut self, state_watcher: &mut StateWatcher) -> TaskNextAction {
        tokio::select! {
            biased;
            _ = state_watcher.while_started() => {
                TaskNextAction::Stop
            }
            _ = self.poll_interval.tick() => {
                tracing::debug!("Polling DaSourceService for block costs");
                let da_block_costs_res = self.process_block_costs().await;
                TaskNextAction::always_continue(da_block_costs_res)
            }
        }
    }

    /// There are no shutdown hooks required by the sources  *yet*
    /// and they should be added here if so, in the future.
    async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}

pub fn new_da_service<S: DaBlockCostsSource>(
    da_source: S,
    poll_interval: Option<Duration>,
    latest_l2_height: Arc<Mutex<BlockHeight>>,
) -> ServiceRunner<DaSourceService<S>> {
    ServiceRunner::new(DaSourceService::new(
        da_source,
        poll_interval,
        latest_l2_height,
        None,
    ))
}
