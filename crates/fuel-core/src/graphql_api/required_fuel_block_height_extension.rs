use crate::graphql_api::database::ReadDatabase;
use async_graphql::{
    extensions::{
        Extension,
        ExtensionContext,
        ExtensionFactory,
        NextPrepareRequest,
    },
    Pos,
    Request,
    ServerError,
    ServerResult,
};
use fuel_core_types::fuel_types::BlockHeight;
use std::sync::Arc;

use super::api_service::REQUIRED_FUEL_BLOCK_HEIGHT_HEADER;

/// The extension that adds the `ReadView` to the request context.
/// It guarantees that the request works with the one view of the database,
/// and external database modification cannot affect the result.
#[derive(Debug, derive_more::Display, derive_more::From)]
pub(crate) struct RequiredFuelBlockHeightExtension;

impl RequiredFuelBlockHeightExtension {
    pub fn new() -> Self {
        Self
    }
}

impl ExtensionFactory for RequiredFuelBlockHeightExtension {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(RequiredFuelBlockHeightExtension::new())
    }
}

pub(crate) struct RequiredFuelBlockHeightTooFarInTheFuture;

#[async_trait::async_trait]
impl Extension for RequiredFuelBlockHeightExtension {
    async fn prepare_request(
        &self,
        ctx: &ExtensionContext<'_>,
        request: Request,
        next: NextPrepareRequest<'_>,
    ) -> ServerResult<Request> {
        let database: &ReadDatabase = ctx.data_unchecked();
        let required_fuel_block_height_header_value = request
            .extensions
            .get(REQUIRED_FUEL_BLOCK_HEIGHT_HEADER)
            .map(|value| match value {
                async_graphql::Value::Number(number) => {
                    // Safety: The value was constructed
                    BlockHeight::new(
                        number
                            .as_u64()
                            .and_then(|n| n.try_into().ok())
                            .expect("The REQUIRED_FUEL_BLOCK_HEIGHT_HEADER value has been constructed from a u64 value"),
                    )
                }
                _ => panic!("The REQUIRED_FUEL_BLOCK_HEIGHT_HEADER value has been constructed from a u64 value"),
            });
            
        if let Some(required_fuel_block_height) = required_fuel_block_height_header_value
        {
            let latest_known_block_height = database
                .view()
                .and_then(|view| view.latest_block_height())
                .map_err(|e| {
                    let (line, column) = (line!(), column!());
                    ServerError::new(
                        e.to_string(),
                        Some(Pos {
                            line: line as usize,
                            column: column as usize,
                        }),
                    )
                })?;
            if required_fuel_block_height > latest_known_block_height {
                return Err(ServerError {
                    message: "".to_string(),
                    locations: vec![],
                    source: Some(Arc::new(RequiredFuelBlockHeightTooFarInTheFuture)),
                    path: vec![],
                    extensions: None,
                });
            }
        }

        next.run(ctx, request).await
    }
}
