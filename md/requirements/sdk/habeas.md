# Putting it together in Habeas

Suppose the body of `check_token` changes after its requirement review was saved.
Running CI reports that the old evidence is stale. Running `habeas author --all`
selects that requirement for another review, gathers its initial inputs, and
invokes the configured agent. The SDK provides the mechanisms described in
[Graph](graph.md) and [Sources](sources.md); Habeas decides which reviews to run
and what their results mean.

## Follow `habeas author --all`

The batch command begins with repository inspection, not model inference.
Habeas discovers the current declarations, references, sites, and evidence;
selects the reviews needing work; and preserves current records unless forced.
It also handles orphaned reviews. When there is nothing to author, it returns
without resolving model configuration or credentials.

```mermaid
flowchart TD
    Start["habeas author --all"] --> Inspect
    subgraph Client["Habeas: batch and review policy"]
        Inspect["Inspect repository and saved evidence"] --> Select["Remove orphaned reviews; select work"]
        Select --> Needed{"Reviews needed?"}
        Needed -->|no| Done["Return without a model"]
        Needed -->|yes| Configure["Resolve provider, policy, concurrency"]
        Configure --> Prepare["Prepare mandatory inputs for each subject"]
        Prepare --> Run["Run the subject's review"]
        Run --> Confirm["Apply confirmation and disagreement policy"]
        Confirm --> Save["Write Habeas evidence and usage records"]
        Save --> Coverage["Review coverage lost by replaced records"]
    end
    subgraph Core["SDK: mechanisms used during a review"]
        Context["AuthorContext and CheckContext"] --> Trace["Track delivered reads and root selections"]
        Trace --> Record["Finish with output and dependencies"]
    end
    Run --> Context
    Record --> Confirm
```

The batch scheduler is Habeas code (`src/authoring/batch.rs`). It knows what a
requirement, citation, or site review is, which stale records need replacement,
and what to do with disagreement. None of that follows merely from having a
graph, so extracting the graph does not transfer these decisions to the SDK.

Inside an individual review, the division is more precise:

```mermaid
sequenceDiagram
    participant H as Habeas review procedure
    participant S as SDK author/check context
    participant D as Habeas domain adapter
    participant J as Habeas judge and tools
    participant M as Model provider
    H->>S: Read nodes and select relationships
    S->>D: Resolve content and fingerprint
    D-->>S: Snapshot
    S-->>H: Content, dependency retained
    H->>S: Request judgment
    S->>J: Judge with tracked check context
    J->>M: Prompt and delivered input pages
    M-->>J: Tool requests or judgment
    J->>S: Record snapshots actually delivered
    J-->>S: Structured judgment
    S-->>H: Judgment result
    H->>H: Interpret result, continue or finish
```

This is the API boundary the extraction is establishing. In the current port,
Habeas prepares inputs using short-lived check contexts, and `AssessmentJudge`
wraps its existing conversation loop. The adapter imports the exact delivered
inputs and requirement reference-set fingerprint into the authoring context.
It does not re-read inputs after the model has seen them to invent a new digest.
The conversation loop, tool presentation, paging, retries, and provider clients
remain Habeas code (`src/authoring/mod.rs` and its submodules).

That delivery distinction matters to the SDK contract: a prepared page that the
model never receives is not an observed input. A model tool can therefore use
Habeas's delivery adapter to record observations, while a deterministic procedure
can read directly through `CheckContext`.

## Follow the CI test

The project's test calls `habeas::validate()`. It inspects the repository and
reports validation failures; it does not enter the authoring path to fill in
missing or stale evidence.

```mermaid
flowchart TD
    Test["Cargo test: habeas::validate()"] --> Discover
    subgraph Client["Habeas: CI policy and evidence format"]
        Discover["Discover declarations, references, sites, and evidence"] --> Forms["Validate forms, endpoint resolution, and record schemas"]
        Forms --> Coverage["Check required coverage, reference sets, and review presence"]
        Coverage --> Inputs["Resolve recorded inputs and compare recorded fields"]
        Inputs --> Verdict["Interpret determinations and acknowledgment consistency"]
        Verdict --> Report["Aggregate findings; fail test if any remain"]
    end
    subgraph Core["SDK used by the current CI path"]
        Check["ResolveInput procedure with CheckContext"] --> Domain["Domain read returns content and fingerprint"]
    end
    Inputs --> Check
    Domain --> Inputs
```

The diagram groups checks by responsibility; the inspector interleaves these
checks as it visits records and references.

Today, `src/evidence.rs`, `src/citations.rs`, and `src/sites.rs` validate Habeas's
JSON schemas, reference sets, coverage, and judgments. Input resolution goes
through `ResolveInput` and the SDK check context in `src/sdk.rs`. Detailed
comparison and diagnostic wording remain in Habeas.

The SDK also provides `Record` and `record_is_current` for generic dependency
checking. The current CI path does **not** deserialize Habeas evidence into
those records and hand all validation to that method. The intended reusable
operation is narrower than a complete Habeas CI result:

```mermaid
flowchart TD
    R["Saved SDK record"] --> F["SDK: compare procedure identity and dependencies"]
    Current["Current domain snapshots"] --> F
    F --> State["Current, stale, or resolution error"]
    State --> H["Consumer: is the saved verdict acceptable?"]
    H --> CI["Consumer's CI result"]
```

A current negative judgment is still negative. A missing review is still
missing even if every existing record is current. Checking freshness is SDK
machinery; deciding which reviews are required and which judgments satisfy the
project is Habeas policy.
