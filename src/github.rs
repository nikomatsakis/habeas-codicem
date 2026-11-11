//! GitHub issue reads. The `github` tool, the issue states supplied with
//! `{fixme}` inputs, and `habeas status` all read through one cache under
//! the state root. Repository validation never reads GitHub.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::ReferenceKind;
use crate::evidence::{RepositoryInspection, SourceReference, display_path};
use crate::fixme::{FixmeBody, is_issue_reference};

/// The GitHub REST API.
const GITHUB_API: &str = "https://api.github.com";

/// Overrides the API base URL with a loopback `http://127.0.0.1:<port>`
/// server, so tests can run against a local fake instead of GitHub.
const API_URL_VARIABLE: &str = "HABEAS_GITHUB_API_URL";

/// How long a cached result is reused instead of contacting GitHub.
const CACHE_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// How long one issue read may take before GitHub counts as unreachable.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// An issue or pull request as GitHub reports it, with its title and body
/// redacted.
// {impl}`requirements.authoring.github_reads`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Issue {
    pub(crate) open: bool,
    pub(crate) title: String,
    pub(crate) body: String,
}

/// The outcome of reading one issue.
// {impl}`requirements.authoring.github_reads`
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IssueRead {
    Found(Issue),
    /// GitHub reports that the issue does not exist.
    Missing,
    /// GitHub could not be reached or did not answer usefully.
    Failed(String),
}

impl IssueRead {
    /// The state supplied to a reviewer: an unreachable issue is assumed
    /// open, and one that does not exist acknowledges no more than a closed
    /// one does.
    // {impl}`requirements.authoring.github_reads`
    pub(crate) fn state(&self) -> &'static str {
        match self {
            Self::Found(issue) if !issue.open => "closed",
            Self::Missing => "closed",
            Self::Found(_) | Self::Failed(_) => "open",
        }
    }

    /// Why the issue no longer counts as open, or `None` while it does or
    /// cannot be read.
    // {impl}`requirements.authoring.github_reads`
    fn not_open(&self) -> Option<&'static str> {
        match self {
            Self::Found(issue) if !issue.open => Some("is closed"),
            Self::Missing => Some("does not exist"),
            Self::Found(_) | Self::Failed(_) => None,
        }
    }
}

/// Where and how GitHub is read; `None` never contacts it.
// {impl}`requirements.authoring.github_reads`
#[derive(Clone, Debug)]
pub struct GithubIssues {
    client: Option<Arc<Client>>,
}

/// Whether a `HABEAS_GITHUB_API_URL` value has the only accepted form,
/// `http://127.0.0.1:<port>`, optionally with a trailing slash.
// {impl}`requirements.authoring.github_reads`
fn is_loopback_api_url(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .map(|rest| rest.strip_suffix('/').unwrap_or(rest))
        .is_some_and(|port| {
            !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok_and(|port| port != 0)
        })
}

/// One configured GitHub API endpoint.
// {impl}`requirements.authoring.github_reads`
#[derive(Debug)]
struct Client {
    api: String,
    /// The cache directory, or `None` when no state root is available.
    cache: Option<PathBuf>,
    token: Option<String>,
}

impl GithubIssues {
    /// Reads nothing: every issue counts as unreachable and so as open.
    // {cite}`habeas:util`
    pub fn offline() -> Self {
        Self { client: None }
    }

    /// Reads GitHub, authenticating with `GITHUB_TOKEN` when it is set and
    /// caching under the state root, for the project at `root`. A state root
    /// inside that project, or one that cannot be determined, leaves reads
    /// uncached. Unit tests never contact GitHub.
    // {impl}`requirements.authoring.github_reads`
    // {impl}`requirements.configuration.state_location`
    pub fn from_env(root: &Path) -> Self {
        if cfg!(test) {
            return Self::offline();
        }
        let api = std::env::var(API_URL_VARIABLE)
            .ok()
            .filter(|url| is_loopback_api_url(url))
            .unwrap_or_else(|| GITHUB_API.to_owned());
        let token = std::env::var("GITHUB_TOKEN")
            .ok()
            .filter(|token| !token.trim().is_empty());
        let cache = crate::config::state_root()
            .ok()
            .filter(|state| {
                fs::canonicalize(root)
                    .and_then(|root| crate::config::state_in_project(state, &root))
                    .is_ok_and(|inside| !inside)
            })
            .map(|state| state.join("github"));
        Self::with_api(api, cache, token)
    }

    // {impl}`requirements.authoring.github_reads`
    pub(crate) fn with_api(api: String, cache: Option<PathBuf>, token: Option<String>) -> Self {
        Self {
            client: Some(Arc::new(Client {
                api: api.trim_end_matches('/').to_owned(),
                cache,
                token,
            })),
        }
    }

    /// Reads one issue reference of the form `<owner>/<repo>#<number>`.
    // {impl}`requirements.authoring.github_reads`
    pub(crate) fn read(&self, issue: &str) -> IssueRead {
        self.read_all([issue.to_owned()])
            .remove(issue)
            .unwrap_or_else(|| IssueRead::Failed("no result".to_owned()))
    }

    /// Reads several issues concurrently, each from a fresh cache entry when
    /// there is one. This blocks; it runs its requests on its own thread so
    /// it may be called from within an asynchronous runtime.
    // {impl}`requirements.authoring.github_reads`
    pub(crate) fn read_all(
        &self,
        issues: impl IntoIterator<Item = String>,
    ) -> BTreeMap<String, IssueRead> {
        let issues = issues.into_iter().collect::<BTreeSet<_>>();
        let Some(client) = &self.client else {
            return issues
                .into_iter()
                .map(|issue| (issue, IssueRead::Failed("GitHub reads are disabled".into())))
                .collect();
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let mut results = BTreeMap::new();
        let mut pending = Vec::new();
        for issue in issues {
            match client.cached(&issue, now) {
                Some(read) => {
                    results.insert(issue, read);
                }
                None => pending.push(issue),
            }
        }
        if pending.is_empty() {
            return results;
        }
        let worker = Arc::clone(client);
        let fetched = std::thread::spawn(move || worker.fetch_all(pending))
            .join()
            .unwrap_or_default();
        for (issue, read) in fetched {
            if !matches!(read, IssueRead::Failed(_)) {
                client.store(&issue, now, &read);
            }
            results.insert(issue, read);
        }
        results
    }
}

impl Client {
    /// The cache file of an issue: owner, repository, and number joined by
    /// `+`, which none of them may contain.
    // {impl}`requirements.authoring.github_reads`
    fn cache_path(&self, issue: &str) -> Option<PathBuf> {
        let (owner, repo, number) = split_issue(issue)?;
        Some(
            self.cache
                .as_ref()?
                .join(format!("{owner}+{repo}+{number}.json")),
        )
    }

    /// A cached result less than an hour old, read from this endpoint.
    // {impl}`requirements.authoring.github_reads`
    fn cached(&self, issue: &str, now: u64) -> Option<IssueRead> {
        let entry: Value =
            serde_json::from_str(&fs::read_to_string(self.cache_path(issue)?).ok()?).ok()?;
        let fetched = entry.get("fetched")?.as_u64()?;
        if entry.get("issue")?.as_str()? != issue
            || entry.get("api")?.as_str()? != self.api
            || fetched > now
            || now - fetched >= CACHE_LIFETIME.as_secs()
        {
            return None;
        }
        let text = |field: &str| {
            entry
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Some(match entry.get("state")?.as_str()? {
            "missing" => IssueRead::Missing,
            state @ ("open" | "closed") => IssueRead::Found(Issue {
                open: state == "open",
                title: text("title"),
                body: text("body"),
            }),
            _ => return None,
        })
    }

    /// Records a definitive result. A failed cache write only means the
    /// next read contacts GitHub again.
    // {impl}`requirements.authoring.github_reads`
    fn store(&self, issue: &str, now: u64, read: &IssueRead) {
        let Some(path) = self.cache_path(issue) else {
            return;
        };
        let mut entry = json!({"issue": issue, "api": self.api, "fetched": now});
        match read {
            IssueRead::Found(found) => {
                entry["state"] = json!(if found.open { "open" } else { "closed" });
                entry["title"] = json!(found.title);
                entry["body"] = json!(found.body);
            }
            IssueRead::Missing => entry["state"] = json!("missing"),
            IssueRead::Failed(_) => return,
        }
        let Some(directory) = path.parent() else {
            return;
        };
        let _ = fs::create_dir_all(directory).and_then(|()| {
            let mut file = tempfile::NamedTempFile::new_in(directory)?;
            file.write_all(entry.to_string().as_bytes())?;
            file.persist(&path).map(|_| ()).map_err(|error| error.error)
        });
    }

    /// Fetches issues concurrently on a runtime of this thread's own.
    // {impl}`requirements.authoring.github_reads`
    fn fetch_all(&self, issues: Vec<String>) -> BTreeMap<String, IssueRead> {
        let failed = |issues: Vec<String>, message: String| {
            issues
                .into_iter()
                .map(|issue| (issue, IssueRead::Failed(message.clone())))
                .collect()
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return failed(issues, error.to_string()),
        };
        let http = match reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("habeas/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(http) => http,
            Err(error) => return failed(issues, error.to_string()),
        };
        runtime.block_on(async {
            let mut requests = tokio::task::JoinSet::new();
            for issue in issues {
                let http = http.clone();
                let url = split_issue(&issue).map(|(owner, repo, number)| {
                    format!("{}/repos/{owner}/{repo}/issues/{number}", self.api)
                });
                let token = self.token.clone();
                requests.spawn(async move {
                    let read = match url {
                        Some(url) => fetch(&http, &url, token.as_deref()).await,
                        None => IssueRead::Failed(format!("{issue:?} is not an issue reference")),
                    };
                    (issue, read)
                });
            }
            let mut results = BTreeMap::new();
            while let Some(result) = requests.join_next().await {
                if let Ok((issue, read)) = result {
                    results.insert(issue, read);
                }
            }
            results
        })
    }
}

/// Reads one issue or pull request: 404 and 410 mean it does not exist;
/// any other failure leaves its state unknown.
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.redaction`
async fn fetch(http: &reqwest::Client, url: &str, token: Option<&str>) -> IssueRead {
    let mut request = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => return IssueRead::Failed(format!("cannot reach GitHub: {error}")),
    };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return IssueRead::Missing;
    }
    if !status.is_success() {
        return IssueRead::Failed(format!("GitHub answered HTTP {status}"));
    }
    let body = match response.text().await {
        Ok(body) => body,
        Err(error) => return IssueRead::Failed(format!("cannot read GitHub's answer: {error}")),
    };
    let Ok(value) = serde_json::from_str::<Value>(&body) else {
        return IssueRead::Failed("GitHub's answer is not JSON".to_owned());
    };
    let open = match value.get("state").and_then(Value::as_str) {
        Some("open") => true,
        Some("closed") => false,
        _ => return IssueRead::Failed("GitHub's answer has no issue state".to_owned()),
    };
    let text = |field: &str| {
        crate::redaction::redact_text(value.get(field).and_then(Value::as_str).unwrap_or(""))
            .into_owned()
    };
    IssueRead::Found(Issue {
        open,
        title: text("title"),
        body: text("body"),
    })
}

/// The owner, repository, and number of a valid issue reference.
// {impl}`requirements.authoring.github_reads`
fn split_issue(issue: &str) -> Option<(&str, &str, &str)> {
    if !is_issue_reference(issue) {
        return None;
    }
    let (repository, number) = issue.split_once('#')?;
    let (owner, repo) = repository.split_once('/')?;
    Some((owner, repo, number))
}

/// The `github` tool: one issue's reference, state, title, and redacted
/// body. Its result is not an evidence input.
// {impl}`requirements.authoring.github_reads`
pub(crate) fn tool(github: &GithubIssues, arguments: &Value) -> Result<Value, String> {
    let object = arguments
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or("github expects exactly an issue field")?;
    let issue = object
        .get("issue")
        .and_then(Value::as_str)
        .ok_or("github expects exactly an issue field")?;
    if !is_issue_reference(issue) {
        return Err(format!(
            "{issue:?} is not an issue reference of the form <owner>/<repo>#<number>"
        ));
    }
    match github.read(issue) {
        IssueRead::Found(found) => Ok(json!({
            "reference": issue,
            "state": if found.open { "open" } else { "closed" },
            "title": found.title,
            "body": found.body,
        })),
        IssueRead::Missing => Err(format!("GitHub issue {issue} does not exist")),
        IssueRead::Failed(error) => Err(format!("cannot read GitHub issue {issue}: {error}")),
    }
}

/// The `github` tool's definition.
// {impl}`requirements.authoring.github_reads`
// {impl}`requirements.authoring.initial_prompt`
pub(crate) fn tool_definition() -> Value {
    json!({
        "name": "github",
        "description": "Read a GitHub issue or pull request named as <owner>/<repo>#<number>, such as one a {fixme} option line names. Returns its reference, state (open or closed), title, and body, with credential-shaped text redacted. Results are not evidence inputs. A malformed reference, an issue that does not exist, or a failed request returns a tool error.",
        "parameters": {"type": "object", "properties": {"issue": {"type": "string"}}, "required": ["issue"], "additionalProperties": false}
    })
}

/// The closed `{fixme}` fences of a reference list and the issues each
/// names.
// {impl}`requirements.authoring.github_reads`
fn fixme_issues(references: &[SourceReference]) -> Vec<(&SourceReference, Vec<String>)> {
    references
        .iter()
        .filter(|reference| reference.kind == ReferenceKind::Fixme && reference.closed)
        .filter_map(|reference| {
            let body = FixmeBody::parse(reference.commentary.as_deref()?);
            (!body.issues.is_empty()).then_some((reference, body.issues))
        })
        .collect()
}

/// The states of the issues named by the `{fixme}` references inside each
/// input's sources, in input order, for supplying with the inputs.
// {impl}`requirements.authoring.github_reads`
pub(crate) fn input_issue_states(
    root: &Path,
    github: &GithubIssues,
    references: &[SourceReference],
    definitions: &BTreeMap<String, crate::evidence::Declaration>,
    inputs: &[Value],
) -> Vec<Vec<(String, &'static str)>> {
    let fixmes = fixme_issues(references);
    let outlines = crate::inputs::OutlineCache::default();
    let named = inputs
        .iter()
        .map(|input| {
            let ranges = input_ranges(root, &outlines, input, definitions);
            let mut issues = Vec::new();
            for (reference, names) in &fixmes {
                let file = display_path(root, &reference.file);
                if ranges.iter().any(|(range_file, start, end)| {
                    *range_file == file && *start <= reference.line && reference.line <= *end
                }) {
                    for issue in names {
                        if !issues.contains(issue) {
                            issues.push(issue.clone());
                        }
                    }
                }
            }
            issues
        })
        .collect::<Vec<_>>();
    if named.iter().all(Vec::is_empty) {
        return named.into_iter().map(|_| Vec::new()).collect();
    }
    let reads = github.read_all(named.iter().flatten().cloned());
    named
        .into_iter()
        .map(|issues| {
            issues
                .into_iter()
                .map(|issue| {
                    let state = reads.get(&issue).map_or("open", IssueRead::state);
                    (issue, state)
                })
                .collect()
        })
        .collect()
}

/// The source lines a model input covers: an outline node's current range,
/// or the declarations a requirements or definition input matches.
// {impl}`requirements.authoring.github_reads`
fn input_ranges(
    root: &Path,
    outlines: &crate::inputs::OutlineCache,
    input: &Value,
    definitions: &BTreeMap<String, crate::evidence::Declaration>,
) -> Vec<(String, usize, usize)> {
    if let (Some(file), Some(item)) = (
        input.get("file").and_then(Value::as_str),
        input.get("item").and_then(Value::as_str),
    ) {
        let parsed = outlines.parse(root, file);
        let Ok(parsed) = parsed.as_ref() else {
            return Vec::new();
        };
        return parsed.resolve(file, item).map_or_else(
            |_| Vec::new(),
            |index| {
                let (range, _) = parsed.node_range(file, index);
                vec![(range.file, range.line_start, range.line_end)]
            },
        );
    }
    let names = if let Some(definition) = input.get("definition").and_then(Value::as_str) {
        vec![definition.to_owned()]
    } else {
        input
            .get("matches")
            .and_then(Value::as_array)
            .map(|matches| {
                matches
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    names
        .iter()
        .filter_map(|name| definitions.get(name))
        .map(|declaration| {
            (
                declaration.file.clone(),
                declaration.line_start,
                declaration.line_end,
            )
        })
        .collect()
}

/// Adds the findings that depend on issue states: each `{fixme}` naming a
/// closed or nonexistent issue and the item containing it, and each FIXME
/// record an acknowledging `{fixme}` of which names one, which is stale.
/// Issues that cannot be read count as open.
// {impl}`requirements.authoring.github_reads`
pub(crate) fn add_issue_findings(inspection: &mut RepositoryInspection, github: &GithubIssues) {
    let root = inspection.root.clone();
    let fixmes = fixme_issues(&inspection.references);
    if fixmes.is_empty() {
        return;
    }
    let reads = github.read_all(fixmes.iter().flat_map(|(_, issues)| issues.iter().cloned()));
    let not_open = |issue: &String| reads.get(issue).and_then(IssueRead::not_open);
    let outlines = crate::inputs::OutlineCache::default();
    let mut errors = BTreeSet::new();
    for (reference, issues) in &fixmes {
        let file = display_path(&root, &reference.file);
        for issue in issues {
            let Some(reason) = not_open(issue) else {
                continue;
            };
            errors.insert(format!(
                "{file}:{}: {{fixme}} names {issue}, which {reason}",
                reference.line
            ));
            let (line, item) = containing_item(inspection, &outlines, &file, reference);
            errors.insert(format!(
                "{file}:{line}: {item} fails: its {{fixme}} at line {} names {issue}, which {reason}",
                reference.line
            ));
        }
    }

    for (name, definition) in &inspection.definitions {
        if definition.kind == crate::DefinitionKind::Term {
            continue;
        }
        let relative = format!("evidence/{name}.json");
        let Some(entries) = fs::read_to_string(root.join(&relative))
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .and_then(|value| {
                crate::judgment::Judgment::from_value(value.get("determination")?).ok()
            })
            .map(|judgment| judgment.fixme_entries().to_vec())
            .filter(|entries| !entries.is_empty())
        else {
            continue;
        };
        for (reference, issues) in &fixmes {
            // A `{fixme}` inside one of the requirement's sites belongs to
            // that site's review.
            if reference.name != *name
                || !reference.is_fixme_edge()
                || crate::sites::in_own_site(&root, &inspection.sites, reference)
            {
                continue;
            }
            let body = FixmeBody::parse(reference.commentary.as_deref().unwrap_or(""));
            if !body
                .acknowledgments()
                .iter()
                .any(|acknowledgment| entries.contains(acknowledgment))
            {
                continue;
            }
            for issue in issues {
                if let Some(reason) = not_open(issue) {
                    errors.insert(format!(
                        "{relative}: stale: issue {issue}, named by the acknowledging {{fixme}} to {name} at {}:{}, {reason}; reassess the record",
                        display_path(&root, &reference.file),
                        reference.line
                    ));
                }
            }
        }
    }
    // A site review's FIXME record is stale when an issue a `{fixme}` inside
    // the site names is closed or missing.
    for site in inspection.sites.values().flatten() {
        let relative = site.review_path();
        let fixme = fs::read_to_string(root.join(&relative))
            .ok()
            .and_then(|content| serde_json::from_str::<Value>(&content).ok())
            .and_then(|value| {
                crate::judgment::Judgment::from_value(value.get("determination")?).ok()
            })
            .is_some_and(|judgment| !judgment.fixme_entries().is_empty());
        if !fixme {
            continue;
        }
        for (reference, issues) in &fixmes {
            if !site
                .fixmes(&root, &inspection.references)
                .iter()
                .any(|inside| inside.file == reference.file && inside.line == reference.line)
            {
                continue;
            }
            for issue in issues {
                if let Some(reason) = not_open(issue) {
                    errors.insert(format!(
                        "{relative}: stale: issue {issue}, named by the {{fixme}} to {} inside the site at {}:{}, {reason}; reassess the review",
                        site.requirement,
                        display_path(&root, &reference.file),
                        reference.line
                    ));
                }
            }
        }
    }
    inspection.report.errors.extend(errors);
}

/// The line and description of the item containing a `{fixme}`: the
/// requirement or example declaration whose body contains it, or else the
/// outline node it belongs to.
fn containing_item(
    inspection: &RepositoryInspection,
    outlines: &crate::inputs::OutlineCache,
    file: &str,
    reference: &SourceReference,
) -> (usize, String) {
    if reference.in_declaration
        && let Some((name, declaration)) = inspection.definitions.iter().find(|(_, d)| {
            d.kind != crate::DefinitionKind::Term
                && d.file == file
                && d.line_start < reference.line
                && reference.line < d.line_end
        })
    {
        return (
            declaration.line_start,
            format!("{} {name}", declaration.kind.noun()),
        );
    }
    if let Ok(parsed) = outlines.parse(&inspection.root, file).as_ref()
        && parsed.outline.profile.language.is_some()
    {
        let node = &parsed.outline.nodes[parsed.outline.owner_at_line(reference.line)];
        if let (false, Some(name)) = (node.kind.is_empty(), node.display_name()) {
            return (node.declaration_line, format!("{} {name}", node.kind));
        }
        if !node.kind.is_empty() {
            return (
                node.declaration_line,
                format!("{} {}", node.kind, node.selector),
            );
        }
    }
    (1, format!("file {file}"))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use super::*;

    /// A local stand-in for the GitHub API: each request's path selects a
    /// status and JSON body, and the request heads are kept.
    // {test}`requirements.authoring.github_reads`
    struct FakeGithub {
        endpoint: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl FakeGithub {
        // {test}`requirements.authoring.github_reads`
        fn start(answers: &[(&str, u16, Value)]) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let answers = answers
                .iter()
                .map(|(path, status, body)| ((*path).to_owned(), (*status, body.to_string())))
                .collect::<BTreeMap<_, _>>();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::clone(&requests);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let mut head = String::new();
                    let mut reader = BufReader::new(&mut stream);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        head.push_str(&line);
                    }
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                    seen.lock().unwrap().push(head);
                    let (status, body) = answers
                        .get(&path)
                        .cloned()
                        .unwrap_or((404, "{}".to_owned()));
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
            Self { endpoint, requests }
        }

        // {test}`requirements.authoring.github_reads`
        fn count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    // {test}`requirements.authoring.github_reads`
    fn issue(state: &str, body: &str) -> Value {
        json!({"state": state, "title": "Queue capacity", "body": body})
    }

    // {test}`requirements.authoring.github_reads`
    #[test]
    fn only_loopback_api_urls_with_a_port_are_accepted() {
        for url in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1:1/",
            "http://127.0.0.1:65535",
        ] {
            assert!(is_loopback_api_url(url), "{url} should be accepted");
        }
        for url in [
            "https://api.github.com",
            "http://example.com:8080",
            "https://127.0.0.1:8080",
            "http://localhost:8080",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:abc",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:+80",
            "http://127.0.0.1:80/api",
            "http://127.0.0.1:80@evil.example/",
            "http://127.0.0.10:80",
        ] {
            assert!(!is_loopback_api_url(url), "{url} should be ignored");
        }
    }

    // {test}`requirements.authoring.github_reads`
    // {test}`requirements.authoring.redaction`
    #[test]
    fn the_tool_reads_issues_with_the_token_and_redacts_bodies() {
        let token = ["ghp_", "abcdefghijklmnopqrstuvwxyz0123456789"].concat();
        let github = FakeGithub::start(&[
            (
                "/repos/example/queue/issues/125",
                200,
                issue("closed", &format!("Leaked {token} here.")),
            ),
            ("/repos/example/queue/issues/7", 200, issue("open", "")),
            ("/repos/example/queue/issues/500", 500, json!({})),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let issues = GithubIssues::with_api(
            github.endpoint.clone(),
            Some(cache.path().to_owned()),
            Some("TEST_TOKEN".to_owned()),
        );
        let read = tool(&issues, &json!({"issue": "example/queue#125"})).unwrap();
        assert_eq!(read["reference"], "example/queue#125");
        assert_eq!(read["state"], "closed");
        assert_eq!(read["title"], "Queue capacity");
        let body = read["body"].as_str().unwrap();
        assert!(
            !body.contains(&token) && body.contains("[REDACTED"),
            "{body}"
        );
        let head = github.requests.lock().unwrap()[0].clone();
        assert!(
            head.starts_with("GET /repos/example/queue/issues/125 "),
            "{head}"
        );
        assert!(head.contains("Bearer TEST_TOKEN"), "{head}");
        // The cache keeps only redacted text.
        let cached = fs::read_to_string(cache.path().join("example+queue+125.json")).unwrap();
        assert!(!cached.contains(&token), "{cached}");
        assert_eq!(
            tool(&issues, &json!({"issue": "example/queue#7"})).unwrap()["state"],
            "open"
        );

        for (arguments, message) in [
            (json!({"issue": "example/queue"}), "not an issue reference"),
            (json!({"issue": "#7"}), "not an issue reference"),
            (json!({"issue": "o/r#1", "extra": 1}), "exactly an issue"),
            (json!({"issue": "example/queue#404"}), "does not exist"),
            (json!({"issue": "example/queue#500"}), "HTTP 500"),
        ] {
            let error = tool(&issues, &arguments).unwrap_err();
            assert!(error.contains(message), "{arguments}: {error}");
        }
        let unreachable = GithubIssues::with_api("http://127.0.0.1:9".to_owned(), None, None);
        let error = tool(&unreachable, &json!({"issue": "example/queue#1"})).unwrap_err();
        assert!(error.contains("cannot read GitHub issue"), "{error}");
        assert_eq!(unreachable.read("example/queue#1").state(), "open");
    }

    // {test}`requirements.authoring.github_reads`
    // {test}`requirements.configuration.state_location`
    #[test]
    fn results_are_cached_by_issue_for_an_hour_and_failures_are_not() {
        let github = FakeGithub::start(&[
            ("/repos/o/r/issues/1", 200, issue("open", "")),
            ("/repos/o/r/issues/3", 503, json!({})),
        ]);
        let cache = tempfile::tempdir().unwrap();
        let issues =
            GithubIssues::with_api(github.endpoint.clone(), Some(cache.path().to_owned()), None);
        let reads = issues.read_all(["o/r#1".to_owned(), "o/r#2".to_owned(), "o/r#3".to_owned()]);
        assert_eq!(reads["o/r#1"].state(), "open");
        assert_eq!(reads["o/r#2"], IssueRead::Missing);
        assert!(matches!(reads["o/r#3"], IssueRead::Failed(_)));
        assert_eq!(github.count(), 3);
        // Definitive results are reused; the failure is retried.
        let again = issues.read_all(["o/r#1".to_owned(), "o/r#2".to_owned(), "o/r#3".to_owned()]);
        assert_eq!(again["o/r#1"], reads["o/r#1"]);
        assert_eq!(again["o/r#2"], IssueRead::Missing);
        assert_eq!(github.count(), 4);
        assert!(!cache.path().join("o+r+3.json").exists());

        // An entry an hour old, or from another endpoint, is read again.
        let path = cache.path().join("o+r+1.json");
        let mut entry: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        entry["fetched"] = json!(entry["fetched"].as_u64().unwrap() - 3600);
        fs::write(&path, entry.to_string()).unwrap();
        issues.read("o/r#1");
        assert_eq!(github.count(), 5);
        issues.read("o/r#1");
        assert_eq!(github.count(), 5);
        let other = FakeGithub::start(&[]);
        let moved =
            GithubIssues::with_api(other.endpoint.clone(), Some(cache.path().to_owned()), None);
        assert_eq!(moved.read("o/r#1"), IssueRead::Missing);
        assert_eq!(other.count(), 1);
    }

    // {test}`requirements.authoring.github_reads`
    // {test}`requirements.references.site_review`
    #[test]
    fn a_closed_issue_of_a_fixme_inside_a_site_makes_only_its_site_review_stale() {
        let github = FakeGithub::start(&[("/repos/o/r/issues/1", 200, issue("closed", ""))]);
        let source = crate::sites::tests::SOURCE.replace(
            "fn check_two() {}",
            "fn check_two() {\n    // :::{fixme} requirements.rule\n    // :github: o/r#1\n    // Panics.\n    // :::\n}",
        );
        let temp = crate::sites::tests::project(crate::sites::tests::RULE, &source);
        let root = temp.path();
        crate::sites::tests::write_review(
            root,
            "fn[check_two]",
            json!({"determination": {"FIXME": ["o/r#1"]}}),
        );
        fs::write(
            root.join("evidence/requirements.rule.json"),
            json!({"determination": {"FIXME": ["o/r#1"]}}).to_string(),
        )
        .unwrap();
        let mut inspection = crate::evidence::inspect_repository(root).unwrap();
        let before = inspection.report.errors.len();
        add_issue_findings(
            &mut inspection,
            &GithubIssues::with_api(github.endpoint.clone(), None, None),
        );
        let added = inspection.report.errors[before..].to_vec();
        let review = crate::sites::review_path("requirements.rule", "src/lib.rs", "fn[check_two]");
        assert!(
            added.contains(&format!(
                "{review}: stale: issue o/r#1, named by the {{fixme}} to requirements.rule inside the site at src/lib.rs:3, is closed; reassess the review"
            )),
            "{added:#?}"
        );
        assert!(
            !added
                .iter()
                .any(|error| error.starts_with("evidence/requirements.rule.json")),
            "{added:#?}"
        );
    }

    /// A project whose `{fixme}`s name `o/r#1` (closed), `o/r#2` (open),
    /// and `o/r#3` (missing): a named one on a function, acknowledged by a
    /// FIXME record, and an unnamed one in a requirement's body.
    // {cite}`habeas:util`
    fn project() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("md")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("evidence")).unwrap();
        fs::write(root.join("book.toml"), "[book]\nsrc = 'md'\n").unwrap();
        fs::write(
            root.join("md/rules.md"),
            "# Rules\n\n::::{req} ready\nReady MUST hold.\n\n:::{fixme}\n:github: o/r#3\n:::\n::::\n\n:::{req} other\nOther MUST hold.\n:::\n",
        )
        .unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "// :::{fixme} rules.ready\n// :github: o/r#1\n// :github: o/r#2\n// Not tested.\n// :::\nfn ready() {}\n\n// :::{fixme} rules.other\n// :github: o/r#2\n// Not tested.\n// :::\nfn other() {}\n",
        )
        .unwrap();
        for name in ["rules.ready", "rules.other"] {
            let issue = if name == "rules.ready" {
                "o/r#1"
            } else {
                "o/r#2"
            };
            fs::write(
                root.join(format!("evidence/{name}.json")),
                json!({"determination": {"FIXME": [issue]}}).to_string(),
            )
            .unwrap();
        }
        temp
    }

    // {test}`requirements.cli.fixme_issues`
    // {test}`requirements.authoring.github_reads`
    #[test]
    fn closed_and_missing_issues_fail_their_fixmes_items_and_acknowledged_records() {
        let github = FakeGithub::start(&[
            ("/repos/o/r/issues/1", 200, issue("closed", "")),
            ("/repos/o/r/issues/2", 200, issue("open", "")),
        ]);
        let temp = project();
        let mut inspection = crate::evidence::inspect_repository(temp.path()).unwrap();
        let before = inspection.report.errors.clone();
        assert_eq!(github.count(), 0, "inspection reads no issues");
        let issues = GithubIssues::with_api(github.endpoint.clone(), None, None);
        add_issue_findings(&mut inspection, &issues);
        let added = inspection.report.errors[before.len()..].to_vec();
        for expected in [
            "src/lib.rs:1: {fixme} names o/r#1, which is closed",
            "src/lib.rs:6: fn ready fails: its {fixme} at line 1 names o/r#1, which is closed",
            "md/rules.md:6: {fixme} names o/r#3, which does not exist",
            "md/rules.md:3: requirement rules.ready fails: its {fixme} at line 6 names o/r#3, which does not exist",
            "evidence/rules.ready.json: stale: issue o/r#1, named by the acknowledging {fixme} to rules.ready at src/lib.rs:1, is closed; reassess the record",
        ] {
            assert!(
                added.iter().any(|error| error == expected),
                "{expected}\n{added:#?}"
            );
        }
        assert_eq!(added.len(), 5, "{added:#?}");
        assert!(!added.iter().any(|error| error.contains("o/r#2")));

        // Unreachable issues count as open, and nothing is added.
        let mut inspection = crate::evidence::inspect_repository(temp.path()).unwrap();
        add_issue_findings(&mut inspection, &GithubIssues::offline());
        assert_eq!(inspection.report.errors, before);
        let mut inspection = crate::evidence::inspect_repository(temp.path()).unwrap();
        let unreachable = GithubIssues::with_api("http://127.0.0.1:9".to_owned(), None, None);
        add_issue_findings(&mut inspection, &unreachable);
        assert_eq!(inspection.report.errors, before);
    }

    // {test}`requirements.authoring.github_reads`
    #[test]
    fn inputs_containing_fixmes_carry_their_issue_states() {
        let github = FakeGithub::start(&[
            ("/repos/o/r/issues/1", 200, issue("closed", "")),
            ("/repos/o/r/issues/2", 200, issue("open", "")),
        ]);
        let temp = project();
        let root = fs::canonicalize(temp.path()).unwrap();
        let mut errors = Vec::new();
        let references = crate::evidence::collect_references(&root, &mut errors);
        let definitions = crate::evidence::load_definitions(&root).unwrap();
        let inputs = [
            json!({"file": "src/lib.rs", "item": "fn[ready]"}),
            json!({"file": "src/lib.rs", "item": "fn[other]"}),
            json!({"requirements": "rules.ready", "matches": ["rules.ready"]}),
            json!({"requirements": "rules.other", "matches": ["rules.other"]}),
        ];
        let issues = GithubIssues::with_api(github.endpoint.clone(), None, None);
        let states = input_issue_states(&root, &issues, &references, &definitions, &inputs);
        let owned = |pairs: &[(&str, &'static str)]| {
            pairs
                .iter()
                .map(|(issue, state)| ((*issue).to_owned(), *state))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            states,
            [
                owned(&[("o/r#1", "closed"), ("o/r#2", "open")]),
                owned(&[("o/r#2", "open")]),
                owned(&[("o/r#3", "closed")]),
                Vec::new(),
            ]
        );
        // Offline, every named issue is assumed open.
        let offline = input_issue_states(
            &root,
            &GithubIssues::offline(),
            &references,
            &definitions,
            &inputs,
        );
        assert_eq!(offline[0], owned(&[("o/r#1", "open"), ("o/r#2", "open")]));
    }
}
