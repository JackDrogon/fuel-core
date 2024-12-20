use crate::{
    fuel_core_graphql_api::{
        api_service::ConsensusProvider,
        query_costs,
    },
    schema::{
        scalars::{
            Address,
            AssetId,
            U128,
        },
        ReadViewProvider,
    },
};
use anyhow::anyhow;
use async_graphql::{
    connection::{
        Connection,
        EmptyFields,
    },
    Context,
    InputObject,
    Object,
};
use fuel_core_types::services::graphql_api;
use futures::StreamExt;

use super::scalars::U64;

pub struct Balance(graphql_api::AddressBalance);

#[Object]
impl Balance {
    async fn owner(&self) -> Address {
        self.0.owner.into()
    }

    async fn amount(&self) -> U64 {
        let amount: u64 = self.0.amount.try_into().unwrap_or(u64::MAX);
        amount.into()
    }

    async fn amount_u128(&self) -> U128 {
        self.0.amount.into()
    }

    async fn asset_id(&self) -> AssetId {
        self.0.asset_id.into()
    }
}

#[derive(InputObject)]
struct BalanceFilterInput {
    /// Filter coins based on the `owner` field
    owner: Address,
}

#[derive(Default)]
pub struct BalanceQuery;

#[Object]
impl BalanceQuery {
    #[graphql(complexity = "query_costs().balance_query")]
    async fn balance(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "address of the owner")] owner: Address,
        #[graphql(desc = "asset_id of the coin")] asset_id: AssetId,
    ) -> async_graphql::Result<Balance> {
        let query = ctx.read_view()?;
        let base_asset_id = *ctx
            .data_unchecked::<ConsensusProvider>()
            .latest_consensus_params()
            .base_asset_id();
        let balance = query
            .balance(owner.0, asset_id.0, base_asset_id)
            .await?
            .into();
        Ok(balance)
    }

    #[graphql(
        complexity = "query_costs().balance_query + query_costs().storage_iterator \
        + (query_costs().storage_read + first.unwrap_or_default() as usize) * child_complexity \
        + (query_costs().storage_read + last.unwrap_or_default() as usize) * child_complexity"
    )]
    async fn balances(
        &self,
        ctx: &Context<'_>,
        filter: BalanceFilterInput,
        first: Option<i32>,
        after: Option<String>,
        last: Option<i32>,
        before: Option<String>,
    ) -> async_graphql::Result<Connection<AssetId, Balance, EmptyFields, EmptyFields>>
    {
        let query = ctx.read_view()?;
        if !query.balances_enabled && (before.is_some() || after.is_some()) {
            return Err(anyhow!(
                "Can not use pagination when balances indexation is not available"
            )
            .into())
        }
        let base_asset_id = *ctx
            .data_unchecked::<ConsensusProvider>()
            .latest_consensus_params()
            .base_asset_id();
        let owner = filter.owner.into();
        crate::schema::query_pagination(after, before, first, last, |start, direction| {
            Ok(query
                .balances(&owner, (*start).map(Into::into), direction, &base_asset_id)
                .map(|result| {
                    result.map(|balance| (balance.asset_id.into(), balance.into()))
                }))
        })
        .await
    }
}

impl From<graphql_api::AddressBalance> for Balance {
    fn from(balance: graphql_api::AddressBalance) -> Self {
        Balance(balance)
    }
}
