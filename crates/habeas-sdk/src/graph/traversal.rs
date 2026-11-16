use super::*;
use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::num::NonZeroUsize;

// {cite}`habeas:util`
pub struct TraversalOptions {
    pub concurrency: NonZeroUsize,
}

impl Default for TraversalOptions {
    // {cite}`habeas:util`
    fn default() -> Self {
        Self {
            concurrency: NonZeroUsize::new(4).unwrap(),
        }
    }
}

// {cite}`habeas:util`
pub struct TraversalResult<N, M, E> {
    pub complete: bool,
    pub nodes: Vec<(N, Result<Judgment<M>, ValidationError<E>>)>,
}

impl<'d, D, F> Validator<'d, D, F>
where
    D: Domain,
    F: Fn(&D::NodeId) -> Result<String, D::Error> + Send + Sync,
{
    // {impl}`requirements.sdk.graph.traversal.validate_transitive`
    // {impl}`requirements.sdk.graph.traversal.validation_cycles`
    // {impl}`requirements.sdk.graph.traversal.validation_parallelism`
    pub async fn validate_transitive(
        &self,
        start: &D::NodeId,
        options: TraversalOptions,
    ) -> TraversalResult<D::NodeId, D::JudgmentMetadata, D::Error> {
        let mut nodes = vec![start.clone()];
        let mut selections = Vec::new();
        let mut adjacency = Vec::new();
        let mut results = Vec::new();
        let mut complete = true;
        let mut index = 0;
        while index < nodes.len() {
            match self.checker.graph.outgoing_edges(&nodes[index]) {
                Ok(snapshot) => {
                    let mut targets = Vec::new();
                    for edge in &snapshot.content {
                        let target = match nodes.iter().position(|node| node == &edge.target) {
                            Some(target) => target,
                            None => {
                                nodes.push(edge.target.clone());
                                nodes.len() - 1
                            }
                        };
                        targets.push(target);
                    }
                    selections.push(Some(snapshot));
                    adjacency.push(targets);
                    results.push(None);
                }
                Err(error) => {
                    complete = false;
                    selections.push(None);
                    adjacency.push(Vec::new());
                    results.push(Some(Err(ValidationError::Graph(error))));
                }
            }
            index += 1;
        }
        let components = components(&adjacency);
        let mut component_of = vec![0; nodes.len()];
        for (component, members) in components.iter().enumerate() {
            for &node in members {
                component_of[node] = component;
            }
        }
        let mut scheduled: Vec<bool> = results.iter().map(Option::is_some).collect();
        let mut active: FuturesUnordered<
            BoxFuture<
                '_,
                (
                    usize,
                    Result<Judgment<D::JudgmentMetadata>, ValidationError<D::Error>>,
                ),
            >,
        > = FuturesUnordered::new();
        loop {
            let done: Vec<bool> = components
                .iter()
                .map(|members| members.iter().all(|&node| results[node].is_some()))
                .collect();
            for node in 0..nodes.len() {
                if active.len() == options.concurrency.get() {
                    break;
                }
                if scheduled[node] {
                    continue;
                }
                let component = component_of[node];
                let ready = components[component].iter().all(|&member| {
                    adjacency[member].iter().all(|&target| {
                        component_of[target] == component || done[component_of[target]]
                    })
                });
                if !ready {
                    continue;
                }
                scheduled[node] = true;
                let id = &nodes[node];
                let edges = selections[node].take();
                active.push(Box::pin(async move {
                    (node, self.validate_with_edges(id, edges).await)
                }));
            }
            match active.next().await {
                Some((node, result)) => results[node] = Some(result),
                None => break,
            }
        }
        drop(active);
        TraversalResult {
            complete,
            nodes: nodes
                .into_iter()
                .zip(
                    results
                        .into_iter()
                        .map(|result| result.expect("every discovered component is scheduled")),
                )
                .collect(),
        }
    }
}

// {cite}`habeas:util`
/// Iterative Kosaraju avoids stack overflow on long reachable chains.
fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::new();
    for start in 0..edges.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if *next == edges[*node].len() {
                order.push(*node);
                stack.pop();
                continue;
            }
            let target = edges[*node][*next];
            *next += 1;
            if !visited[target] {
                visited[target] = true;
                stack.push((target, 0));
            }
        }
    }
    let mut reverse = vec![Vec::new(); edges.len()];
    for (node, targets) in edges.iter().enumerate() {
        for &target in targets {
            reverse[target].push(node);
        }
    }
    visited.fill(false);
    let mut result = Vec::new();
    for &start in order.iter().rev() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![start];
        let mut members = Vec::new();
        while let Some(node) = stack.pop() {
            members.push(node);
            for &target in &reverse[node] {
                if !visited[target] {
                    visited[target] = true;
                    stack.push(target);
                }
            }
        }
        result.push(members);
    }
    result
}
