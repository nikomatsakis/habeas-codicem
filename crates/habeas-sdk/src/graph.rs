//! Consumer-defined graphs and tracked, model-free reads.
//!
//! Resource loading, fingerprints, graph meaning, and persistence belong to the
//! consumer. A snapshot pairs content with the fingerprint of that exact view.

/// The saved value used to detect changes to a review input.
///
/// For example, a consumer can hash a function's UTF-8 text with SHA-256 and
/// save the digest here. CI recomputes that fingerprint to decide whether an
/// earlier review still applies. Equality compares both fields; it says nothing
/// about whether the reviewed code is correct.
///
/// `scheme` names the rules used to compute `value`, such as
/// `function-text-sha256-v1`. Hashing only the function's signature needs a
/// different scheme: it ignores changes that a whole-function review relies on.
/// The consumer supplies these rules; the SDK treats both fields as opaque.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub scheme: String,
    pub value: String,
}

/// Content from one read, paired with the fingerprint of that content.
///
/// Resolve both together so a changing resource cannot pair old content with a
/// fingerprint computed from a newer read. `T` can be text, a parsed structure,
/// or another consumer value; it need not have a filename or source span.
#[derive(Clone, Debug)]
pub struct Snapshot<T> {
    pub content: T,
    pub fingerprint: Fingerprint,
}

/// An edge owns its typed endpoints. Implementations normally match an enum;
/// there is no separately stored, potentially inconsistent endpoint pair.
pub trait Edge<N> {
    fn endpoints(&self) -> (N, N);
}

// ANCHOR: domain
// {impl}`requirements.sdk.graph.domain_types`
pub trait Domain {
    type NodeId: Clone + Eq;
    type Edge: Edge<Self::NodeId>;
    type Content;
    type Query: Clone + Eq;
    type QueryResult;
    type Error;

    fn read(&self, node: &Self::NodeId) -> Result<Snapshot<Self::Content>, Self::Error>;
    /// Fingerprint the complete result, including empty selections. Adding an
    /// edge to an empty selection must change this fingerprint.
    fn query(&self, query: &Self::Query) -> Result<Snapshot<Self::QueryResult>, Self::Error>;
}
// ANCHOR_END: domain

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dependency<N, Q> {
    Node(N, Fingerprint),
    Query(Q, Fingerprint),
}

/// A delivery/read log. Distinct observations of the same identity are kept:
/// if it changes during a procedure, no current state can satisfy both.
#[derive(Clone, Debug)]
pub struct Trace<T> {
    observations: Vec<T>,
}

impl<T> Default for Trace<T> {
    fn default() -> Self {
        Self {
            observations: Vec::new(),
        }
    }
}

impl<T: PartialEq> Trace<T> {
    pub fn observe(&mut self, observation: T) {
        if !self.observations.contains(&observation) {
            self.observations.push(observation);
        }
    }

    pub fn observations(&self) -> &[T] {
        &self.observations
    }
}

/// Model-free access to a domain. No provider is stored here, including when
/// this context is borrowed from an authoring context.
///
/// ```compile_fail
/// use habeas_sdk::graph::{CheckContext, Domain};
/// fn cannot_invoke_model<D: Domain>(ctx: &mut CheckContext<'_, D>) {
///     ctx.judge(());
/// }
/// ```
// {impl}`requirements.sdk.graph.model_capability`
// {impl}`requirements.sdk.graph.deterministic_procedures`
pub struct CheckContext<'a, D: Domain> {
    domain: &'a D,
    trace: Trace<Dependency<D::NodeId, D::Query>>,
}

impl<'a, D: Domain> CheckContext<'a, D> {
    pub fn new(domain: &'a D) -> Self {
        Self {
            domain,
            trace: Trace::default(),
        }
    }

    pub fn read(&mut self, node: &D::NodeId) -> Result<D::Content, D::Error> {
        Ok(self.read_snapshot(node)?.content)
    }

    // {impl}`requirements.sdk.graph.node_inputs`
    pub fn read_snapshot(&mut self, node: &D::NodeId) -> Result<Snapshot<D::Content>, D::Error> {
        let snapshot = self.domain.read(node)?;
        self.observe_read(node.clone(), snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    pub fn query(&mut self, query: &D::Query) -> Result<D::QueryResult, D::Error> {
        Ok(self.query_snapshot(query)?.content)
    }

    pub fn query_snapshot(
        &mut self,
        query: &D::Query,
    ) -> Result<Snapshot<D::QueryResult>, D::Error> {
        let snapshot = self.domain.query(query)?;
        self.observe_query(query.clone(), snapshot.fingerprint.clone());
        Ok(snapshot)
    }

    /// Import the exact selection snapshot supplied by a consumer-owned tool.
    pub fn observe_query(&mut self, query: D::Query, fingerprint: Fingerprint) {
        self.trace.observe(Dependency::Query(query, fingerprint));
    }

    /// Import a read delivered by a consumer-owned tool adapter. The adapter
    /// must supply the fingerprint of the delivered snapshot, not re-read the
    /// resource here. Imported human attestations should use a separate record.
    // {impl}`requirements.sdk.graph.node_inputs`
    pub fn observe_read(&mut self, node: D::NodeId, fingerprint: Fingerprint) {
        self.trace.observe(Dependency::Node(node, fingerprint));
    }

    pub fn dependencies(&self) -> &[Dependency<D::NodeId, D::Query>] {
        self.trace.observations()
    }

    /// Re-resolve a captured dependency without invoking its authoring procedure.
    /// Missing/unreadable content is an error, not evidence of freshness.
    pub fn is_current(
        &mut self,
        dependency: &Dependency<D::NodeId, D::Query>,
    ) -> Result<bool, D::Error> {
        let current = match dependency {
            Dependency::Node(node, previous) => {
                let snapshot = self.domain.read(node)?;
                let current = &snapshot.fingerprint == previous;
                self.trace
                    .observe(Dependency::Node(node.clone(), snapshot.fingerprint));
                current
            }
            Dependency::Query(query, previous) => {
                let snapshot = self.domain.query(query)?;
                let current = &snapshot.fingerprint == previous;
                self.trace
                    .observe(Dependency::Query(query.clone(), snapshot.fingerprint));
                current
            }
        };
        Ok(current)
    }
}

// ANCHOR: check-procedure
pub trait CheckProcedure<D: Domain> {
    type Request;
    type Output;

    fn run(
        &self,
        request: &Self::Request,
        ctx: &mut CheckContext<'_, D>,
    ) -> Result<Self::Output, D::Error>;
}
// ANCHOR_END: check-procedure

/// A completed procedure and the observations supporting its output. The
/// consumer owns serialization, history, and the meaning of the output.
pub struct Record<N, Q, V> {
    pub procedure: String,
    pub dependencies: Vec<Dependency<N, Q>>,
    pub output: V,
}

impl<'a, D: Domain> CheckContext<'a, D> {
    /// Seals the observations without inferring any domain verdict.
    pub fn finish<V>(self, procedure: String, output: V) -> Record<D::NodeId, D::Query, V> {
        Record {
            procedure,
            dependencies: self.trace.observations,
            output,
        }
    }

    /// Compare procedure identity and all captured snapshots. A caller should
    /// version its procedure identity when its algorithm or prompts change.
    // {impl}`requirements.sdk.graph.checking_records`
    pub fn record_is_current<V>(
        &mut self,
        procedure: &str,
        record: &Record<D::NodeId, D::Query, V>,
    ) -> Result<bool, D::Error> {
        let mut current = procedure == record.procedure;
        for dependency in &record.dependencies {
            current &= self.is_current(dependency)?;
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<u8, String>>);
    enum Link {
        Related(u8, u8),
    }
    impl Edge<u8> for Link {
        fn endpoints(&self) -> (u8, u8) {
            let Self::Related(a, b) = self;
            (*a, *b)
        }
    }
    impl Domain for Memory {
        type NodeId = u8;
        type Edge = Link;
        type Content = String;
        type Query = ();
        type QueryResult = Vec<u8>;
        type Error = &'static str;
        fn read(&self, node: &u8) -> Result<Snapshot<String>, Self::Error> {
            let content = self.0.borrow().get(node).cloned().ok_or("missing")?;
            Ok(Snapshot {
                fingerprint: Fingerprint {
                    scheme: "text-v1".into(),
                    value: content.clone(),
                },
                content,
            })
        }
        fn query(&self, _: &()) -> Result<Snapshot<Vec<u8>>, Self::Error> {
            let content: Vec<_> = self.0.borrow().keys().copied().collect();
            Ok(Snapshot {
                fingerprint: Fingerprint {
                    scheme: "keys-v1".into(),
                    value: format!("{content:?}"),
                },
                content,
            })
        }
    }

    #[test]
    fn observes_empty_queries_and_changes_during_a_procedure() {
        let domain = Memory::default();
        let mut ctx = CheckContext::new(&domain);
        assert_eq!(ctx.query(&()).unwrap(), Vec::<u8>::new());
        domain.0.borrow_mut().insert(1, "before".into());
        assert_eq!(ctx.read(&1).unwrap(), "before");
        assert_eq!(ctx.read(&1).unwrap(), "before");
        domain.0.borrow_mut().insert(1, "after".into());
        assert_eq!(ctx.read(&1).unwrap(), "after");
        assert_eq!(ctx.dependencies().len(), 3);
        let record = ctx.finish("review-v1".into(), true);
        assert!(
            !CheckContext::new(&domain)
                .record_is_current("review-v1", &record)
                .unwrap()
        );
    }

    // {test}`requirements.sdk.graph.node_inputs`
    // {test}`requirements.sdk.graph.checking_records`
    #[test]
    fn missing_reads_do_not_count_as_successful_reads() {
        let domain = Memory::default();
        let mut ctx = CheckContext::new(&domain);
        assert_eq!(ctx.read(&1), Err("missing"));
        assert!(ctx.dependencies().is_empty());
        domain.0.borrow_mut().insert(1, "present".into());
        ctx.read(&1).unwrap();
        let record = ctx.finish("review-v1".into(), ());
        assert!(
            CheckContext::new(&domain)
                .record_is_current("review-v1", &record)
                .unwrap()
        );
        assert!(
            !CheckContext::new(&domain)
                .record_is_current("review-v2", &record)
                .unwrap()
        );
        domain.0.borrow_mut().clear();
        assert_eq!(
            CheckContext::new(&domain).record_is_current("review-v1", &record),
            Err("missing")
        );
    }

    #[test]
    fn fingerprint_interpretation_is_part_of_freshness() {
        let domain = Memory::default();
        domain.0.borrow_mut().insert(1, "same digest".into());
        let old = Dependency::Node(
            1,
            Fingerprint {
                scheme: "different-view".into(),
                value: "same digest".into(),
            },
        );
        assert!(!CheckContext::new(&domain).is_current(&old).unwrap());
        assert_eq!(Link::Related(1, 2).endpoints(), (1, 2));
    }

    // {test}`requirements.sdk.graph.procedures`
    // {test}`requirements.sdk.graph.deterministic_procedures`
    // {test}`requirements.sdk.graph.checking_records`
    #[test]
    fn authoring_can_interleave_checks_and_judgments_and_ci_never_calls_the_judge() {
        use crate::procedure::{AuthorContext, AuthorProcedure, Judge};
        use std::{
            cell::Cell,
            future::Future,
            pin::pin,
            task::{Context, Poll, Waker},
        };
        struct Model<'a>(&'a Cell<usize>);
        impl Judge<Memory> for Model<'_> {
            type Request = u8;
            type Output = usize;
            async fn judge(
                &mut self,
                node: u8,
                ctx: &mut CheckContext<'_, Memory>,
            ) -> Result<usize, &'static str> {
                self.0.set(self.0.get() + 1);
                Ok(ctx.read(&node)?.len())
            }
        }
        struct Review;
        impl AuthorProcedure<Memory, Model<'_>> for Review {
            type Request = u8;
            type Output = bool;
            async fn run(
                &self,
                node: &u8,
                ctx: &mut AuthorContext<'_, Memory, Model<'_>>,
            ) -> Result<bool, &'static str> {
                let first = ctx.judge(*node).await?;
                let second = ctx.judge(*node + 1).await?;
                Ok(first < second && ctx.checks().query(&())?.len() == 2)
            }
        }
        let domain = Memory::default();
        domain
            .0
            .borrow_mut()
            .extend([(1, "short".into()), (2, "longer".into())]);
        let calls = Cell::new(0);
        let mut ctx = AuthorContext::new(&domain, Model(&calls));
        let result = {
            let mut future = pin!(Review.run(&1, &mut ctx));
            let Poll::Ready(result) = future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("test judge does not suspend")
            };
            result.unwrap()
        };
        let record = ctx.finish("review-v1".into(), result);
        assert!(record.output);
        assert_eq!(record.dependencies.len(), 3);
        assert!(
            CheckContext::new(&domain)
                .record_is_current("review-v1", &record)
                .unwrap()
        );
        assert_eq!(calls.get(), 2);
    }
}
