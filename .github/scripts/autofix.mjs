import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const label = 'autofix';
const marker = 'habeas-autofix';

function requireValue(value, message) {
  if (!value) throw new Error(message);
  return value;
}

function number(value) {
  const result = Number(value);
  if (!Number.isSafeInteger(result) || result <= 0) throw new Error(`Invalid number: ${value}`);
  return result;
}

function sha(value) {
  if (!/^[0-9a-f]{40}$/.test(value ?? '')) throw new Error(`Invalid commit SHA: ${value}`);
  return value;
}

function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }).trim();
}

function branch(issue) {
  return `codex/autofix/issue-${issue}`;
}

function metadata(body) {
  const match = body?.match(/<!-- habeas-autofix (\{[^\n]*\}) -->/);
  if (!match) return null;
  try {
    return JSON.parse(match[1]);
  } catch {
    return null;
  }
}

function apiClient(token, apiUrl = 'https://api.github.com') {
  return async (path, init = {}) => {
    const response = await fetch(new URL(path, apiUrl), {
      ...init,
      headers: {
        Accept: 'application/vnd.github+json',
        Authorization: `Bearer ${token}`,
        'X-GitHub-Api-Version': '2026-03-10',
        ...(init.body ? { 'Content-Type': 'application/json' } : {}),
      },
    });
    if (response.status === 404 && init.allow404) return null;
    if (!response.ok) throw new Error(`GitHub API ${response.status} for ${path}: ${(await response.text()).slice(0, 500)}`);
    if (response.status === 204) return null;
    return response.json();
  };
}

async function allPages(api, path) {
  const result = [];
  for (let page = 1; page <= 20; page++) {
    const separator = path.includes('?') ? '&' : '?';
    const items = await api(`${path}${separator}per_page=100&page=${page}`);
    result.push(...items);
    if (items.length < 100) return result;
  }
  throw new Error(`Too many pages from ${path}`);
}

function repoPath(repo, suffix) {
  return `/repos/${repo.full_name}${suffix}`;
}

async function isOwner(api, repo, user) {
  if (!user || user.type !== 'User') return false;
  if (repo.owner.type === 'User') return user.id === repo.owner.id;
  if (repo.owner.type !== 'Organization') return false;
  const membership = await api(`/orgs/${repo.owner.login}/memberships/${user.login}`, { allow404: true });
  return membership?.state === 'active' && membership?.role === 'admin';
}

async function managedPR(api, repo, pr, botLogin) {
  if (!pr || pr.state !== 'open' || pr.user?.login !== botLogin) return null;
  if (pr.head?.repo?.id !== repo.id || pr.base?.ref !== repo.default_branch) return null;
  const meta = metadata(pr.body);
  if (!meta || meta.version !== 2 || !Number.isSafeInteger(meta.issue) || meta.issue <= 0) return null;
  if (pr.head.ref !== branch(meta.issue)) return null;
  // Store mode with the commit, so a push and its state are atomic. The PR body
  // identifies the managed PR but cannot accidentally retain an old mode.
  const commit = await api(repoPath(repo, `/git/commits/${sha(pr.head.sha)}`));
  const state = metadata(commit.message);
  if (!state || state.issue !== meta.issue || !['requirements', 'implementation'].includes(state.stage)) return null;
  if (state.stage === 'implementation') sha(state.requirements_sha);
  return state;
}

async function treeAtCommit(api, repo, commitSha) {
  const commit = await api(repoPath(repo, `/git/commits/${sha(commitSha)}`));
  const tree = await api(repoPath(repo, `/git/trees/${sha(commit.tree.sha)}?recursive=1`));
  if (tree.truncated) throw new Error(`Git tree truncated at ${commitSha}`);
  return tree.tree;
}

async function safePRDiff(api, repo, pr, stage) {
  const files = await allPages(api, repoPath(repo, `/pulls/${pr.number}/files`));
  const entries = new Map((await treeAtCommit(api, repo, pr.head.sha)).map(item => [item.path, item]));
  const allowed = stage === 'requirements'
    ? proposalPathAllowed
    : path => proposalPathAllowed(path) || implementationPathAllowed(path);
  return files.every(file =>
    allowed(file.filename) &&
    (!file.previous_filename || allowed(file.previous_filename)) &&
    (file.status === 'removed' || (
      entries.get(file.filename)?.type === 'blob' &&
      (entries.get(file.filename)?.mode === '100644' ||
        (stage === 'implementation' && !proposalPathAllowed(file.filename) &&
          entries.get(file.filename)?.mode === '100755'))
    ))
  );
}

async function sameMdTree(api, repo, firstSha, secondSha) {
  async function mdTree(commitSha) {
    return (await treeAtCommit(api, repo, commitSha)).filter(item => item.path.startsWith('md/'))
      .map(item => `${item.path}\t${item.mode}\t${item.sha}`).sort();
  }
  const [first, second] = await Promise.all([mdTree(firstSha), mdTree(secondSha)]);
  return JSON.stringify(first) === JSON.stringify(second);
}

async function findPR(api, repo, issue) {
  const head = encodeURIComponent(`${repo.owner.login}:${branch(issue)}`);
  const prs = await allPages(api, repoPath(repo, `/pulls?state=all&head=${head}`));
  return prs.find(pr => pr.head?.ref === branch(issue) && pr.head?.repo?.id === repo.id) ?? null;
}

async function approvedOnHead(api, repo, pr, ownerReviewId = null) {
  const reviews = await allPages(api, repoPath(repo, `/pulls/${pr.number}/reviews`));
  const latest = new Map();
  for (const review of reviews) {
    if (!review.submitted_at || !review.user) continue;
    if (['APPROVED', 'CHANGES_REQUESTED', 'DISMISSED'].includes(review.state)) {
      latest.set(review.user.id, review);
    }
  }
  for (const review of latest.values()) {
    if (review.state === 'CHANGES_REQUESTED' && await isOwner(api, repo, review.user)) return false;
  }
  const approvals = [...latest.values()].filter(review =>
    review.state === 'APPROVED' &&
    review.commit_id === pr.head.sha &&
    (ownerReviewId === null || review.id === ownerReviewId)
  );
  for (const review of approvals) {
    if (await isOwner(api, repo, review.user)) return true;
  }
  return false;
}

function ownerLinks(text, repo) {
  const owner = repo.owner.login.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const name = repo.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const pattern = new RegExp(`https://github\\.com/${owner}/${name}/(?:issues|pull)/\\d+#(issuecomment-|discussion_r)(\\d+)`, 'gi');
  return [...(text ?? '').matchAll(pattern)].slice(0, 10).map(match => ({
    kind: match[1] === 'issuecomment-' ? 'issue' : 'review',
    id: number(match[2]),
    url: match[0],
  }));
}

async function linkedComments(api, repo, ownerTexts) {
  const refs = ownerTexts.flatMap(text => ownerLinks(text, repo));
  const seen = new Set();
  const result = [];
  for (const ref of refs) {
    const key = `${ref.kind}:${ref.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const path = ref.kind === 'issue'
      ? repoPath(repo, `/issues/comments/${ref.id}`)
      : repoPath(repo, `/pulls/comments/${ref.id}`);
    const comment = await api(path);
    if (comment.html_url !== ref.url) throw new Error(`Linked comment URL does not match ${ref.url}`);
    result.push({
      url: comment.html_url,
      author: comment.user.login,
      body: comment.body,
    });
  }
  return result;
}

async function discussionHistory(api, repo, issue, prNumber, botLogin) {
  const history = [];
  async function add(kind, items) {
    for (const item of items) {
      history.push({ kind, url: item.html_url, author: item.user?.login,
        role: await isOwner(api, repo, item.user) ? 'owner' : item.user?.login === botLogin ? 'agent' : 'context',
        body: item.body ?? '', state: item.state, commit: item.commit_id,
        path: item.path, line: item.line, originalLine: item.original_line,
        replyTo: item.in_reply_to_id, createdAt: item.submitted_at ?? item.created_at });
    }
  }
  await add('issue', [issue]);
  await add('issue-comment', await allPages(api, repoPath(repo, `/issues/${issue.number}/comments`)));
  if (prNumber) {
    await add('pr-comment', await allPages(api, repoPath(repo, `/issues/${prNumber}/comments`)));
    await add('review', await allPages(api, repoPath(repo, `/pulls/${prNumber}/reviews`)));
    await add('inline-comment', await allPages(api, repoPath(repo, `/pulls/${prNumber}/comments`)));
  }
  return history;
}

async function requirementsStillApproved(api, repo, pr, requirementsSha) {
  const reviews = await allPages(api, repoPath(repo, `/pulls/${pr.number}/reviews`));
  // Later requests to fix code do not revoke specification approval. Dismissal
  // of the original approval does, as does publishing a new requirements commit.
  for (const review of reviews) {
    if (review.state === 'APPROVED' && review.commit_id === requirementsSha &&
        await isOwner(api, repo, review.user)) return true;
  }
  return false;
}

async function planForEvent(api, eventName, event, botLogin) {
  const repo = event.repository;
  requireValue(repo?.full_name, 'Missing repository');
  requireValue(botLogin, 'Missing bot login');
  const sender = event.sender;
  if (!await isOwner(api, repo, sender)) return null;

  const common = {
    repo: repo.full_name,
    defaultBranch: repo.default_branch,
    actor: sender.login,
    eventName,
    eventAction: event.action,
  };

  if (eventName === 'issues' && ['opened', 'labeled'].includes(event.action)) {
    const issueNumber = number(event.issue?.number);
    if (event.action === 'labeled' && event.label?.name !== label) return null;
    const issue = await api(repoPath(repo, `/issues/${issueNumber}`));
    if (issue.state !== 'open' || !issue.labels?.some(item => item.name === label)) return null;
    if (await findPR(api, repo, issueNumber)) return null;
    const defaultRef = await api(repoPath(repo, `/git/ref/heads/${repo.default_branch}`));
    const ownerTexts = await isOwner(api, repo, issue.user) ? [issue.body] : [];
    return {
      ...common,
      stage: 'requirements',
      trigger: `issue:${issueNumber}:${event.action}`,
      issue: issueNumber,
      issueTitle: issue.title,
      issueBody: issue.body ?? '',
      feedback: null,
      linked: await linkedComments(api, repo, ownerTexts),
      baseSha: sha(defaultRef.object.sha),
      branch: branch(issueNumber),
      prNumber: null,
      requirementsSha: null,
      history: await discussionHistory(api, repo, issue, null, botLogin),
    };
  }

  let prNumber;
  let feedback = null;
  let reviewId = null;
  let approval = false;
  let ownerTexts = [];
  let trigger;

  if (eventName === 'issue_comment' && event.action === 'created' && event.issue?.pull_request) {
    prNumber = number(event.issue.number);
    const comment = await api(repoPath(repo, `/issues/comments/${number(event.comment.id)}`));
    if (comment.user?.id !== sender.id) return null;
    feedback = { kind: 'conversation', url: comment.html_url, body: comment.body ?? '' };
    ownerTexts = [comment.body];
    trigger = `comment:${comment.id}`;
  } else if (eventName === 'pull_request_review' && event.action === 'submitted') {
    prNumber = number(event.pull_request?.number);
    reviewId = number(event.review?.id);
    const review = await api(repoPath(repo, `/pulls/${prNumber}/reviews/${reviewId}`));
    if (review.user?.id !== sender.id) return null;
    approval = review.state === 'APPROVED';
    if (!['APPROVED', 'COMMENTED', 'CHANGES_REQUESTED'].includes(review.state)) return null;
    const inline = await allPages(api, repoPath(repo, `/pulls/${prNumber}/reviews/${reviewId}/comments`));
    const selected = inline.filter(item => item.user?.id === sender.id)
      .map(item => ({ url: item.html_url, path: item.path, line: item.line, body: item.body }));
    feedback = { kind: 'review', url: review.html_url, state: review.state, body: review.body ?? '', inline: selected };
    ownerTexts = [review.body, ...selected.map(item => item.body)];
    trigger = `review:${review.id}`;
  } else {
    return null;
  }

  const pr = await api(repoPath(repo, `/pulls/${prNumber}`));
  const meta = await managedPR(api, repo, pr, botLogin);
  if (!meta) return null;
  // The cumulative PR diff includes both modes; each new commit is gated separately.
  if (!await safePRDiff(api, repo, pr, 'implementation')) return null;
  const issue = await api(repoPath(repo, `/issues/${meta.issue}`));
  if (issue.state !== 'open' || !issue.labels?.some(item => item.name === label)) return null;

  let stage = meta.stage;
  let requirementsSha = meta.requirements_sha ?? null;
  if (approval) {
    if (stage !== 'requirements' || event.review.commit_id !== pr.head.sha) return null;
    if (!await approvedOnHead(api, repo, pr, reviewId)) return null;
    stage = 'implementation';
    requirementsSha = sha(pr.head.sha);
  } else {
    if (!feedback.body && (!feedback.inline || feedback.inline.length === 0)) return null;
    // An approval may have produced only an answer, without a new commit.
    if (stage === 'requirements' && await approvedOnHead(api, repo, pr)) {
      stage = 'implementation';
      requirementsSha = sha(pr.head.sha);
    }
    if (stage === 'implementation') {
      if (!await requirementsStillApproved(api, repo, pr, requirementsSha)) return null;
      if (!await sameMdTree(api, repo, requirementsSha, pr.head.sha)) return null;
    }
  }

  return {
    ...common,
    stage,
    trigger,
    issue: meta.issue,
    issueTitle: issue.title,
    issueBody: issue.body ?? '',
    feedback,
    linked: await linkedComments(api, repo, ownerTexts),
    baseSha: sha(pr.head.sha),
    branch: pr.head.ref,
    prNumber: pr.number,
    requirementsSha,
    history: await discussionHistory(api, repo, issue, prNumber, botLogin),
  };
}

function proposalPathAllowed(path) {
  if (!path || path.includes('..') || path.includes('\\')) return false;
  return /^md\/.+\.md$/.test(path) || /^evidence\/[^/]+\.json$/.test(path);
}

function implementationPathAllowed(path) {
  if (!path || path.startsWith('/') || path.includes('..') || path.includes('\\')) return false;
  if (['.github/', '.agents/', '.claude/', '.cargo/', '.codex/', 'agent-sops/', 'md/', '.git/']
    .some(prefix => path.startsWith(prefix))) return false;
  if (['.gitmodules', '.gitattributes', 'book.toml'].includes(path)) return false;
  return true;
}

function verifyPaths(stage, paths) {
  if (paths.length === 0) return;
  const allowed = stage === 'requirements' ? proposalPathAllowed : implementationPathAllowed;
  for (const path of paths) {
    if (!allowed(path)) throw new Error(`Disallowed ${stage} path: ${path}`);
  }
}

function pathsFromGit() {
  return git('diff', '--cached', '--no-renames', '--name-only', '-z').split('\0').filter(Boolean);
}

function checkModes(stage) {
  const listing = git('ls-files', '-s', '-z').split('\0').filter(Boolean);
  for (const item of listing) {
    const match = /^(\d{6}) [0-9a-f]{40} \d\t(.*)$/.exec(item);
    if (!match) throw new Error(`Invalid index entry: ${item}`);
    if (!pathsFromGit().includes(match[2])) continue;
    if (match[1] !== '100644' && !(stage === 'implementation' && match[1] === '100755')) {
      throw new Error(`Disallowed file mode for ${match[2]}: ${match[1]}`);
    }
  }
}

function modeBadge(stage) {
  return stage === 'requirements' ? '📝 Adjust requirements' : '🛠 Author code';
}

function publicationResult(plan, result, paths) {
  if (!['requirements', 'implementation'].includes(result.stage)) throw new Error('Invalid output mode');
  if (plan.stage === 'requirements' && result.stage !== 'requirements') throw new Error('Requirements need owner approval');
  verifyPaths(result.stage, paths);
  if (result.stage !== plan.stage && !paths.some(path => path.startsWith('md/'))) {
    throw new Error('Returning to requirements must include a proposal');
  }
  if (typeof result.intent !== 'string' || !result.intent.trim()) throw new Error('Missing change intent');
  // Never accept agent-authored state markers in the prose portion of a commit.
  if (JSON.stringify(result).includes(marker)) throw new Error('Reserved state marker');
  const lines = [`${modeBadge(result.stage)}: ${result.intent.trim()}`];
  if (result.stage === 'requirements') {
    if (!Array.isArray(result.sources) || !result.sources.length) throw new Error('Missing quoted sources');
    const sources = [...plan.history, ...plan.linked];
    for (const source of result.sources) {
      const original = sources.find(item => item.url === source.url && item.role !== 'agent');
      if (!original || typeof source.quote !== 'string' || !source.quote.trim() || !original.body.includes(source.quote)) {
        throw new Error('Source must quote the supplied discussion exactly');
      }
      lines.push(`Source: ${source.url} (${original.author})\n> ${source.quote}`);
    }
  } else {
    sha(plan.requirementsSha);
    if (!Array.isArray(result.requirements) || !result.requirements.length) throw new Error('Missing requirements and examples');
    for (const requirement of result.requirements) {
      if (!/^[\w.-]+$/.test(requirement.id ?? '') || typeof requirement.description !== 'string' || !requirement.description.trim()) {
        throw new Error('Each requirement/example needs an ID and description');
      }
      lines.push(`${requirement.id}: ${requirement.description}`);
    }
    lines.push(`Approved requirements: https://github.com/${plan.repo}/commit/${plan.requirementsSha}`);
  }
  const state = { issue: plan.issue, stage: result.stage,
    ...(result.stage === 'implementation' ? { requirements_sha: plan.requirementsSha } : {}) };
  lines.push(`<!-- ${marker} ${JSON.stringify(state)} -->`);
  return { stage: result.stage, commitMessage: lines.join('\n\n') };
}

async function prepare() {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const api = apiClient(requireValue(process.env.GH_TOKEN, 'Missing GH_TOKEN'), process.env.GITHUB_API_URL);
  const plan = await planForEvent(api, process.env.GITHUB_EVENT_NAME, event, process.env.BOT_LOGIN);
  const output = requireValue(process.env.GITHUB_OUTPUT, 'Missing GITHUB_OUTPUT');
  if (!plan) {
    writeFileSync(output, 'run=false\n', { flag: 'a' });
    return;
  }
  const dir = join(process.env.RUNNER_TEMP, 'autofix');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'plan.json'), JSON.stringify(plan, null, 2));
  const sop = plan.stage === 'requirements'
    ? 'agent-sops/requirements-proposal.sop.md'
    : 'agent-sops/approved-implementation.sop.md';
  const prompt = `${readFileSync('agent-sops/autofix-context.md', 'utf8')}\n\n${readFileSync(sop, 'utf8')}\n\n## Provided context_json\n\n\`\`\`json\n${JSON.stringify(plan, null, 2)}\n\`\`\`\n`;
  writeFileSync(join(dir, 'prompt.md'), prompt);
  writeFileSync(output, `run=true\nbase_sha=${plan.baseSha}\nstage=${plan.stage}\n`, { flag: 'a' });
}

async function publish() {
  const artifact = join(process.env.RUNNER_TEMP, 'autofix');
  const planned = JSON.parse(readFileSync(join(artifact, 'plan.json'), 'utf8'));
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  const api = apiClient(requireValue(process.env.GH_TOKEN, 'Missing GH_TOKEN'), process.env.GITHUB_API_URL);
  const current = await planForEvent(api, process.env.GITHUB_EVENT_NAME, event, process.env.BOT_LOGIN);
  if (JSON.stringify(current) !== JSON.stringify(planned)) throw new Error('Autofix state changed while the agent ran');

  const patch = join(artifact, 'changes.patch');
  const report = readFileSync(join(artifact, 'result.md'), 'utf8').trim().slice(0, 58000);
  const result = JSON.parse(readFileSync(join(artifact, 'output.json'), 'utf8'));
  let summary;
  const repo = event.repository;
  const apiRoot = repoPath(repo, '');
  execFileSync('gh', ['auth', 'setup-git'], { stdio: 'inherit', env: process.env });
  git('fetch', 'origin', planned.baseSha);
  git('checkout', '--detach', planned.baseSha);
  if (readFileSync(patch).length > 0) {
    execFileSync('git', ['apply', '--check', '--index', patch], { stdio: 'inherit' });
    execFileSync('git', ['apply', '--index', patch], { stdio: 'inherit' });
    const paths = pathsFromGit();
    const publication = publicationResult(planned, result, paths);
    summary = `**${modeBadge(publication.stage)}**\n\n${report}`;
    checkModes(publication.stage);
    execFileSync('git', ['diff', '--cached', '--check'], { stdio: 'inherit' });
    if (paths.length === 0) throw new Error('Patch produced no changed files');
    git('-c', `user.name=${process.env.BOT_LOGIN}`, '-c', `user.email=${process.env.BOT_LOGIN}@users.noreply.github.com`,
      'commit', '-m', publication.commitMessage);
    const expected = planned.prNumber ? planned.baseSha : '';
    execFileSync('git', ['push', `--force-with-lease=refs/heads/${planned.branch}:${expected}`,
      'origin', `HEAD:refs/heads/${planned.branch}`], { stdio: 'inherit' });

    if (planned.prNumber) {
      if (summary) await api(`${apiRoot}/issues/${planned.prNumber}/comments`, {
        method: 'POST', body: JSON.stringify({ body: summary }),
      });
    } else {
      const meta = { version: 2, issue: planned.issue };
      const title = `Resolve #${planned.issue}: ${planned.issueTitle}`;
      const body = `<!-- ${marker} ${JSON.stringify(meta)} -->\n\n` +
        `Work for #${planned.issue}. Requirements and code are separate commits on this PR.\n\n` +
        `Approve the current requirements head to enable coding. New requirements need another approval. Merging stays manual.\n\n` + summary;
      await api(`${apiRoot}/pulls`, {
        method: 'POST',
        body: JSON.stringify({
          title: title.slice(0, 250),
          head: planned.branch,
          base: planned.defaultBranch,
          body,
          draft: false,
        }),
      });
    }
  } else if (planned.prNumber) {
    publicationResult(planned, result, []);
    summary = `**${modeBadge(planned.stage)}**\n\n${report}`;
    await api(`${apiRoot}/issues/${planned.prNumber}/comments`, {
      method: 'POST', body: JSON.stringify({ body: summary }),
    });
  } else {
    throw new Error('Initial autofix run produced no changes');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const command = process.argv[2];
  try {
    if (command === 'prepare') await prepare();
    else if (command === 'publish') await publish();
    else throw new Error(`Unknown command: ${command}`);
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}

export { isOwner, ownerLinks, proposalPathAllowed, implementationPathAllowed, verifyPaths, planForEvent, approvedOnHead, publicationResult, modeBadge };
