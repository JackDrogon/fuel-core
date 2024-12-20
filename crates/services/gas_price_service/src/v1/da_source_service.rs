use crate::v1::da_source_service::service::DaBlockCostsSource;
use std::{
    ops::RangeInclusive,
    time::Duration,
};

pub mod block_committer_costs;
#[cfg(test)]
pub mod dummy_costs;
pub mod service;

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct DaBlockCosts {
    pub bundle_id: u32,
    pub l2_blocks: RangeInclusive<u32>,
    pub bundle_size_bytes: u32,
    pub blob_cost_wei: u128,
}

#[allow(non_snake_case)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1::da_source_service::{
        dummy_costs::DummyDaBlockCosts,
        service::new_da_service,
    };
    use fuel_core_services::Service;
    use std::{
        sync::{
            Arc,
            Mutex,
        },
        time::Duration,
    };

    #[tokio::test]
    async fn run__when_da_block_cost_source_gives_value_shared_state_is_updated() {
        // given
        let expected_da_cost = DaBlockCosts {
            bundle_id: 1,
            l2_blocks: 0..=9,
            bundle_size_bytes: 1024 * 128,
            blob_cost_wei: 2,
        };
        let notifier = Arc::new(tokio::sync::Notify::new());
        let da_block_costs_source =
            DummyDaBlockCosts::new(Ok(expected_da_cost.clone()), notifier.clone());
        let latest_l2_height = Arc::new(Mutex::new(10u32));
        let service = new_da_service(
            da_block_costs_source,
            Some(Duration::from_millis(1)),
            latest_l2_height,
        );
        let mut shared_state = &mut service.shared.subscribe();

        // when
        service.start_and_await().await.unwrap();
        notifier.notified().await;

        // then
        let actual_da_cost = shared_state.try_recv().unwrap();
        assert_eq!(actual_da_cost, expected_da_cost);
        service.stop_and_await().await.unwrap();
    }

    #[tokio::test]
    async fn run__when_da_block_cost_source_errors_shared_state_is_not_updated() {
        // given
        let notifier = Arc::new(tokio::sync::Notify::new());
        let da_block_costs_source =
            DummyDaBlockCosts::new(Err(anyhow::anyhow!("boo!")), notifier.clone());
        let latest_l2_height = Arc::new(Mutex::new(0u32));
        let service = new_da_service(
            da_block_costs_source,
            Some(Duration::from_millis(1)),
            latest_l2_height,
        );
        let mut shared_state = &mut service.shared.subscribe();

        // when
        service.start_and_await().await.unwrap();
        notifier.notified().await;

        // then
        let da_block_costs_res = shared_state.try_recv();
        assert!(da_block_costs_res.is_err());
        service.stop_and_await().await.unwrap();
    }

    #[tokio::test]
    async fn run__will_not_return_cost_bundles_for_bundles_that_are_greater_than_l2_height(
    ) {
        // given
        let l2_height = 4;
        let unexpected_costs = DaBlockCosts {
            bundle_id: 1,
            l2_blocks: 0..=9,
            bundle_size_bytes: 1024 * 128,
            blob_cost_wei: 2,
        };
        assert!(unexpected_costs.l2_blocks.end() > &l2_height);
        let notifier = Arc::new(tokio::sync::Notify::new());
        let da_block_costs_source =
            DummyDaBlockCosts::new(Ok(unexpected_costs.clone()), notifier.clone());
        let latest_l2_height = Arc::new(Mutex::new(l2_height));
        let service = new_da_service(
            da_block_costs_source,
            Some(Duration::from_millis(1)),
            latest_l2_height,
        );
        let mut shared_state = &mut service.shared.subscribe();

        // when
        service.start_and_await().await.unwrap();
        notifier.notified().await;

        // then
        let err = shared_state.try_recv();
        tracing::info!("err: {:?}", err);
        assert!(err.is_err());
    }
}
