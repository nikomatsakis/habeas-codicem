use super::{AgentConfig, AgentWithTools, Tool};
use crate::{domain::Domain, request::ValidationContext};
use futures::future::BoxFuture;
use rig_core::completion::ToolDefinition;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{io, marker::PhantomData, sync::Arc};

pub trait RegisteredTool<D: Domain>: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> io::Result<ToolDefinition>;
    fn call<'a>(
        &'a self,
        cx: &'a ValidationContext<'_, D>,
        args: &'a Value,
    ) -> BoxFuture<'a, io::Result<String>>;
}

pub(super) struct TypedTool<T, A, R> {
    pub(super) tool: Arc<T>,
    pub(super) signature: PhantomData<fn(A) -> R>,
}

impl<D: Domain, A: DeserializeOwned + JsonSchema, R: Serialize, T: Tool<D, A, R>> RegisteredTool<D>
    for TypedTool<T, A, R>
{
    fn name(&self) -> &str {
        self.tool.name()
    }
    fn definition(&self) -> io::Result<ToolDefinition> {
        let name = rig_core::message::ToolName::new(self.name()).map_err(io::Error::other)?;
        Ok(ToolDefinition::new(
            name,
            self.tool.description(),
            serde_json::to_value(schemars::schema_for!(A))?,
        ))
    }
    fn call<'a>(
        &'a self,
        cx: &'a ValidationContext<'_, D>,
        args: &'a Value,
    ) -> BoxFuture<'a, io::Result<String>> {
        Box::pin(async move {
            let args = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_string(&self.tool.execute(cx, args).await)?)
        })
    }
}

pub(crate) mod sealed {
    use super::*;
    #[doc(hidden)]
    pub trait Settings<D: Domain>: Send + Sync {
        fn into_parts(self) -> (AgentConfig, Vec<Arc<dyn RegisteredTool<D>>>);
    }
    impl<D: Domain> Settings<D> for AgentConfig {
        fn into_parts(self) -> (AgentConfig, Vec<Arc<dyn RegisteredTool<D>>>) {
            (self, Vec::new())
        }
    }
    impl<D: Domain> Settings<D> for AgentWithTools<D> {
        fn into_parts(self) -> (AgentConfig, Vec<Arc<dyn RegisteredTool<D>>>) {
            (self.backend, self.tools)
        }
    }
}
