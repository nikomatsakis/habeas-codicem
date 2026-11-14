//! Explicit authoring capability, independent of any model vendor or runtime.

use crate::graph::{CheckContext, Domain};

// ANCHOR: judge
#[allow(async_fn_in_trait)]
pub trait Judge<D: Domain> {
    type Request;
    type Output;

    /// Implementations expose model tools through this same tracked context.
    async fn judge(
        &mut self,
        request: Self::Request,
        ctx: &mut CheckContext<'_, D>,
    ) -> Result<Self::Output, D::Error>;
}
// ANCHOR_END: judge

/// Constructing this context requires an explicit judge. A check context alone
/// cannot acquire one. Consumers still control arbitrary Rust I/O; this is an
/// API capability boundary, not a sandbox.
// {impl}`requirements.sdk.graph.procedures`
// {impl}`requirements.sdk.graph.model_capability`
pub struct AuthorContext<'a, D: Domain, J: Judge<D>> {
    checks: CheckContext<'a, D>,
    judge: J,
}

impl<'a, D: Domain, J: Judge<D>> AuthorContext<'a, D, J> {
    pub fn new(domain: &'a D, judge: J) -> Self {
        Self {
            checks: CheckContext::new(domain),
            judge,
        }
    }

    pub fn checks(&mut self) -> &mut CheckContext<'a, D> {
        &mut self.checks
    }

    pub fn finish<V>(
        self,
        procedure: String,
        output: V,
    ) -> crate::graph::Record<D::NodeId, D::Query, V> {
        self.checks.finish(procedure, output)
    }

    pub async fn judge(&mut self, request: J::Request) -> Result<J::Output, D::Error> {
        self.judge.judge(request, &mut self.checks).await
    }
}

// ANCHOR: author-procedure
#[allow(async_fn_in_trait)]
pub trait AuthorProcedure<D: Domain, J: Judge<D>> {
    type Request;
    type Output;

    async fn run(
        &self,
        request: &Self::Request,
        ctx: &mut AuthorContext<'_, D, J>,
    ) -> Result<Self::Output, D::Error>;
}
// ANCHOR_END: author-procedure
