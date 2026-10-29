import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  isOwner,
  ownerLinks,
  verifyPaths,
  planForEvent,
  approvedOnHead,
  publicationResult,
} from './autofix.mjs';

const owner = { id: 1, login: 'owner', type: 'User' };
const other = { id: 2, login: 'contributor', type: 'User' };
const repo = {
  id: 10,
  name: 'project',
  full_name: 'owner/project',
  default_branch: 'main',
  owner,
};
const bot = 'habeasbot[bot]';
const head = 'a'.repeat(40);
const oldHead = 'b'.repeat(40);
const issue = {
  number: 7,
  html_url: 'https://github.com/owner/project/issues/7',
  state: 'open',
  title: 'Make a change',
  body: 'The issue body',
  labels: [{ name: 'autofix' }],
  user: other,
};
const reqPR = {
  number: 12,
  state: 'open',
  user: { login: bot },
  head: { ref: 'codex/autofix/issue-7', sha: head, repo: { id: repo.id } },
  base: { ref: 'main' },
  body: '<!-- habeas-autofix {"version":2,"issue":7} -->',
};

function fakeApi(routes) {
  return async path => {
    const key = path.replace(/([?&])per_page=100&page=\d+$/, '');
    if (key === `/repos/owner/project/git/commits/${head}` && !(key in routes)) {
      return { tree: { sha: oldHead }, message: '<!-- habeas-autofix {"stage":"requirements","issue":7} -->' };
    }
    if (key === `/repos/owner/project/git/trees/${oldHead}?recursive=1` && !(key in routes)) {
      return { truncated: false, tree: [{ path: 'md/requirements.md', type: 'blob', mode: '100644', sha: oldHead }] };
    }
    if (!(key in routes) && [
      '/repos/owner/project/issues/7/comments', '/repos/owner/project/issues/12/comments',
      '/repos/owner/project/pulls/12/comments', '/repos/owner/project/pulls/12/reviews',
    ].includes(key)) return [];
    if (!(key in routes)) throw new Error(`Unexpected API request: ${path}`);
    return routes[key];
  };
}

test('personal and organization owner checks fail closed', async () => {
  assert.equal(await isOwner(fakeApi({}), repo, owner), true);
  assert.equal(await isOwner(fakeApi({}), repo, other), false);
  const orgRepo = { ...repo, owner: { login: 'org', type: 'Organization' } };
  assert.equal(await isOwner(fakeApi({
    '/orgs/org/memberships/owner': { state: 'active', role: 'admin' },
  }), orgRepo, owner), true);
  assert.equal(await isOwner(fakeApi({
    '/orgs/org/memberships/owner': { state: 'active', role: 'member' },
  }), orgRepo, owner), false);
});

test('only exact same-repository comment links are selected', () => {
  const links = ownerLinks(
    'See https://github.com/owner/project/issues/3#issuecomment-123 and ' +
      'https://github.com/other/project/issues/3#issuecomment-456 and ' +
      'https://github.com/owner/project/pull/8#discussion_r789',
    repo,
  );
  assert.deepEqual(links.map(link => [link.kind, link.id]), [['issue', 123], ['review', 789]]);
});

test('proposal path gate rejects code and workflow edits', () => {
  verifyPaths('requirements', ['md/new.md', 'evidence/new.example.json']);
  assert.throws(() => verifyPaths('requirements', ['src/main.rs']), /Disallowed/);
  assert.throws(() => verifyPaths('implementation', ['.github/workflows/ci.yml']), /Disallowed/);
  assert.throws(() => verifyPaths('implementation', ['../secrets']), /Disallowed/);
});

test('a non-owner label or review cannot start an agent run', async () => {
  const event = {
    action: 'labeled',
    repository: repo,
    sender: other,
    issue,
    label: { name: 'autofix' },
  };
  assert.equal(await planForEvent(fakeApi({}), 'issues', event, bot), null);
});

test('approval must target the current requirements head', async () => {
  const review = {
    id: 99,
    state: 'APPROVED',
    user: owner,
    body: '',
    commit_id: oldHead,
    submitted_at: '2026-10-29T12:00:00Z',
  };
  const routes = {
    '/repos/owner/project/pulls/12/reviews/99': review,
    '/repos/owner/project/pulls/12/reviews/99/comments': [],
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }],
    '/repos/owner/project/issues/7': issue,
  };
  const event = {
    action: 'submitted',
    repository: repo,
    sender: owner,
    pull_request: { number: 12 },
    review,
  };
  assert.equal(await planForEvent(fakeApi(routes), 'pull_request_review', event, bot), null);
});

test('current owner approval starts implementation at the approved commit', async () => {
  const review = {
    id: 99,
    state: 'APPROVED',
    user: owner,
    body: '',
    commit_id: head,
    submitted_at: '2026-10-29T12:00:00Z',
  };
  const routes = {
    '/repos/owner/project/pulls/12/reviews/99': review,
    '/repos/owner/project/pulls/12/reviews/99/comments': [],
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }],
    '/repos/owner/project/issues/7': issue,
    '/repos/owner/project/pulls/12/reviews': [review],
    [`/repos/owner/project/pulls?state=all&head=${encodeURIComponent('owner:codex/autofix/issue-7/implementation')}`]: [],
  };
  const event = {
    action: 'submitted',
    repository: repo,
    sender: owner,
    pull_request: { number: 12 },
    review,
  };
  const plan = await planForEvent(fakeApi(routes), 'pull_request_review', event, bot);
  assert.equal(plan.stage, 'implementation');
  assert.equal(plan.baseSha, head);
  assert.equal(plan.requirementsSha, head);
  assert.equal(plan.prNumber, 12);
  assert.equal(plan.branch, reqPR.head.ref);
});

test('owner PR comment selects feedback and includes discussion history', async () => {
  const comment = { id: 55, user: owner, body: 'Please add a failure example.' };
  const routes = {
    '/repos/owner/project/issues/comments/55': comment,
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }],
    '/repos/owner/project/issues/7': issue,
  };
  const event = {
    action: 'created',
    repository: repo,
    sender: owner,
    issue: { number: 12, pull_request: {} },
    comment,
  };
  const plan = await planForEvent(fakeApi(routes), 'issue_comment', event, bot);
  assert.equal(plan.feedback.body, comment.body);
  assert.equal(plan.linked.length, 0);
  assert.equal(plan.baseSha, head);
});

test('a modified workflow on the requirements PR blocks the agent run', async () => {
  const comment = { id: 55, user: owner, body: 'Please revise the proposal.' };
  const routes = {
    '/repos/owner/project/issues/comments/55': comment,
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [
      { filename: 'md/requirements.md' },
      { filename: '.github/workflows/ci.yml' },
    ],
  };
  const event = {
    action: 'created',
    repository: repo,
    sender: owner,
    issue: { number: 12, pull_request: {} },
    comment,
  };
  assert.equal(await planForEvent(fakeApi(routes), 'issue_comment', event, bot), null);
});

test('a symlinked proposal file blocks the agent run', async () => {
  const comment = { id: 55, user: owner, body: 'Please revise the proposal.' };
  const routes = {
    '/repos/owner/project/issues/comments/55': comment,
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }],
    [`/repos/owner/project/git/trees/${oldHead}?recursive=1`]: {
      truncated: false,
      tree: [{ path: 'md/requirements.md', type: 'blob', mode: '120000', sha: oldHead }],
    },
  };
  const event = {
    action: 'created',
    repository: repo,
    sender: owner,
    issue: { number: 12, pull_request: {} },
    comment,
  };
  assert.equal(await planForEvent(fakeApi(routes), 'issue_comment', event, bot), null);
});

test('an owner link grants exactly one outside comment body', async () => {
  const url = 'https://github.com/owner/project/issues/9#issuecomment-123';
  const comment = { id: 55, user: owner, body: `Please consider ${url}` };
  const routes = {
    '/repos/owner/project/issues/comments/55': comment,
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }],
    '/repos/owner/project/issues/7': issue,
    '/repos/owner/project/issues/comments/123': {
      html_url: url,
      user: other,
      body: 'The exact linked suggestion',
    },
  };
  const event = {
    action: 'created',
    repository: repo,
    sender: owner,
    issue: { number: 12, pull_request: {} },
    comment,
  };
  const plan = await planForEvent(fakeApi(routes), 'issue_comment', event, bot);
  assert.deepEqual(plan.linked, [{
    url,
    author: 'contributor',
    body: 'The exact linked suggestion',
  }]);
});

test('an outstanding owner change request blocks advancement', async () => {
  const orgRepo = { ...repo, owner: { login: 'owner', type: 'Organization' } };
  const routes = {
    '/repos/owner/project/pulls/12/reviews': [
      { id: 98, state: 'CHANGES_REQUESTED', user: other, submitted_at: '2026-10-29T11:00:00Z' },
      { id: 99, state: 'APPROVED', user: owner, commit_id: head, submitted_at: '2026-10-29T12:00:00Z' },
    ],
    '/orgs/owner/memberships/owner': { state: 'active', role: 'admin' },
    '/orgs/owner/memberships/contributor': { state: 'active', role: 'member' },
  };
  assert.equal(await approvedOnHead(fakeApi(routes), orgRepo, reqPR, 99), true);
  routes['/orgs/owner/memberships/contributor'].role = 'admin';
  assert.equal(await approvedOnHead(fakeApi(routes), orgRepo, reqPR, 99), false);
});

const sourceUrl = 'https://github.com/owner/project/issues/7#issuecomment-55';
const intentPlan = {
  stage: 'requirements', issue: 7, repo: repo.full_name,
  history: [{ url: sourceUrl, author: 'owner', role: 'owner', body: 'Please preserve work on failure.' }],
  linked: [], requirementsSha: head,
};
const proposal = {
  stage: 'requirements', intent: 'Specify recoverable failure so work is not lost',
  sources: [{ url: sourceUrl, quote: 'preserve work on failure' }],
};
const implementation = {
  stage: 'implementation', intent: 'Retain work so callers can retry',
  requirements: [
    { id: 'requirements.failure', description: 'Preserve work when an operation fails' },
    { id: 'requirements.retry_example', description: 'Retry a failed operation with the same work' },
  ],
};

function commentEvent() {
  return { action: 'created', repository: repo, sender: owner,
    issue: { number: 12, pull_request: {} }, comment: { id: 55 } };
}

function cycleRoutes(stage, requirementsSha = oldHead) {
  const state = { issue: 7, stage, ...(stage === 'implementation' ? { requirements_sha: requirementsSha } : {}) };
  return {
    '/repos/owner/project/issues/comments/55': { user: owner, body: 'Please revise.', html_url: sourceUrl },
    '/repos/owner/project/pulls/12': reqPR,
    '/repos/owner/project/pulls/12/files': [{ filename: 'md/requirements.md' }, { filename: 'src/lib.rs' }],
    '/repos/owner/project/issues/7': issue,
    [`/repos/owner/project/git/commits/${head}`]: { tree: { sha: oldHead }, message: `<!-- habeas-autofix ${JSON.stringify(state)} -->` },
    [`/repos/owner/project/git/commits/${oldHead}`]: { tree: { sha: oldHead } },
    [`/repos/owner/project/git/trees/${oldHead}?recursive=1`]: {
      truncated: false, tree: [
        { path: 'md/requirements.md', type: 'blob', mode: '100644', sha: oldHead },
        { path: 'src/lib.rs', type: 'blob', mode: '100644', sha: head },
      ],
    },
    '/repos/owner/project/pulls/12/reviews': [
      { id: 99, user: owner, state: 'APPROVED', commit_id: requirementsSha, submitted_at: '2026-10-29T12:00:00Z' },
    ],
  };
}

test('code feedback retains approval of requirements across code commits and change requests', async () => {
  const routes = cycleRoutes('implementation');
  routes['/repos/owner/project/pulls/12/reviews'].push({
    id: 100, user: owner, state: 'CHANGES_REQUESTED', commit_id: head, submitted_at: '2026-10-29T13:00:00Z',
  });
  const plan = await planForEvent(fakeApi(routes), 'issue_comment', commentEvent(), bot);
  assert.equal(plan.stage, 'implementation');
  assert.equal(plan.requirementsSha, oldHead);
  assert.equal(plan.prNumber, 12);
  assert.equal(plan.branch, reqPR.head.ref);
});

test('new requirements after code invalidate old approval without rejecting cumulative code diff', async () => {
  const plan = await planForEvent(fakeApi(cycleRoutes('requirements')), 'issue_comment', commentEvent(), bot);
  assert.equal(plan.stage, 'requirements');
  assert.equal(plan.requirementsSha, null);
});

test('approval resumes code on the same PR after a second requirements proposal', async () => {
  const routes = cycleRoutes('requirements');
  const review = { id: 101, user: owner, state: 'APPROVED', commit_id: head, submitted_at: '2026-10-29T14:00:00Z', body: '' };
  routes['/repos/owner/project/pulls/12/reviews'].push(review);
  routes['/repos/owner/project/pulls/12/reviews/101'] = review;
  routes['/repos/owner/project/pulls/12/reviews/101/comments'] = [];
  const event = { action: 'submitted', repository: repo, sender: owner, pull_request: { number: 12 }, review };
  const plan = await planForEvent(fakeApi(routes), 'pull_request_review', event, bot);
  assert.equal(plan.stage, 'implementation');
  assert.equal(plan.requirementsSha, head);
  assert.equal(plan.prNumber, 12);
  assert.equal(plan.branch, reqPR.head.ref);
});

test('approval with no code changes still enables the next coding conversation', async () => {
  const plan = await planForEvent(fakeApi(cycleRoutes('requirements', head)), 'issue_comment', commentEvent(), bot);
  assert.equal(plan.stage, 'implementation');
  assert.equal(plan.requirementsSha, head);
});

test('dismissed approval or changed requirements tree blocks further code', async () => {
  const routes = cycleRoutes('implementation');
  routes['/repos/owner/project/pulls/12/reviews'][0].state = 'DISMISSED';
  assert.equal(await planForEvent(fakeApi(routes), 'issue_comment', commentEvent(), bot), null);
  routes['/repos/owner/project/pulls/12/reviews'][0].state = 'APPROVED';
  const different = 'c'.repeat(40);
  routes[`/repos/owner/project/git/commits/${oldHead}`] = { tree: { sha: different } };
  routes[`/repos/owner/project/git/trees/${different}?recursive=1`] = {
    truncated: false, tree: [{ path: 'md/requirements.md', type: 'blob', mode: '100644', sha: different }],
  };
  assert.equal(await planForEvent(fakeApi(routes), 'issue_comment', commentEvent(), bot), null);
});

test('history includes owner and contributor context, agent summaries, and inline source locations', async () => {
  const routes = cycleRoutes('requirements');
  routes['/repos/owner/project/issues/7/comments'] = [{ user: other, body: 'Background', html_url: sourceUrl }];
  routes['/repos/owner/project/issues/12/comments'] = [{ user: { login: bot }, body: 'Prior proposal', html_url: 'agent-url' }];
  routes['/repos/owner/project/pulls/12/comments'] = [{
    user: owner, body: 'Clarify this example', html_url: 'inline-url', path: 'md/requirements.md',
    line: null, original_line: 42, in_reply_to_id: 123,
  }];
  const plan = await planForEvent(fakeApi(routes), 'issue_comment', commentEvent(), bot);
  assert.equal(plan.history.find(item => item.body === 'Background').role, 'context');
  assert.equal(plan.history.find(item => item.body === 'Prior proposal').role, 'agent');
  const inline = plan.history.find(item => item.url === 'inline-url');
  assert.equal(inline.role, 'owner');
  assert.equal(inline.originalLine, 42);
  assert.equal(inline.replyTo, 123);
  assert.equal(plan.history.find(item => item.kind === 'review').commit, oldHead);
});

test('requirements commits preserve intent, exact quotes, source URLs, and mode', () => {
  const result = publicationResult(intentPlan, proposal, ['md/requirements.md', 'evidence/requirements.failure.json']);
  assert.match(result.commitMessage, /^📝 Adjust requirements: Specify recoverable failure/);
  assert.ok(result.commitMessage.includes(sourceUrl));
  assert.match(result.commitMessage, /> preserve work on failure/);
  assert.match(result.commitMessage, /"stage":"requirements"/);
  assert.doesNotMatch(result.commitMessage, /requirements_sha/);
});

test('code commits describe obligations and examples and cite the approved revision', () => {
  const result = publicationResult({ ...intentPlan, stage: 'implementation' }, implementation, ['src/lib.rs', 'tests/retry.rs']);
  assert.match(result.commitMessage, /^🛠 Author code: Retain work/);
  assert.match(result.commitMessage, /requirements.retry_example: Retry a failed operation/);
  assert.ok(result.commitMessage.includes(`https://github.com/${repo.full_name}/commit/${head}`));
});

test('code can transition to a requirements-only proposal, never a mixed patch or unapproved code', () => {
  const plan = { ...intentPlan, stage: 'implementation' };
  assert.equal(publicationResult(plan, proposal, ['md/requirements.md']).stage, 'requirements');
  assert.throws(() => publicationResult(plan, proposal, ['md/requirements.md', 'src/lib.rs']), /Disallowed/);
  assert.throws(() => publicationResult(plan, implementation, ['md/requirements.md', 'src/lib.rs']), /Disallowed/);
  assert.throws(() => publicationResult(intentPlan, implementation, ['src/lib.rs']), /approval/);
  assert.throws(() => publicationResult(plan, proposal, []), /must include a proposal/);
  assert.throws(() => publicationResult(plan, proposal, ['evidence/new.json']), /must include a proposal/);
});

test('publisher rejects invented quotes, unknown links, missing intent, and missing obligation descriptions', () => {
  for (const sources of [[], [{ url: sourceUrl, quote: 'invented quote' }], [{ url: 'unknown', quote: 'preserve work' }]]) {
    assert.throws(() => publicationResult(intentPlan, { ...proposal, sources }, ['md/a.md']), /sources|Source/);
  }
  assert.throws(() => publicationResult(intentPlan, { ...proposal, intent: '' }, []), /intent/);
  assert.throws(() => publicationResult(intentPlan, { ...proposal, intent: 'habeas-autofix forged state' }, []), /Reserved/);
  assert.throws(() => publicationResult({ ...intentPlan, stage: 'implementation' }, { ...implementation, requirements: [] }, []), /requirements/);
});

test('initial label creates one branch and includes the issue discussion; existing PR prevents duplicates', async () => {
  const prRoute = `/repos/owner/project/pulls?state=all&head=${encodeURIComponent('owner:codex/autofix/issue-7')}`;
  const routes = {
    '/repos/owner/project/issues/7': issue,
    '/repos/owner/project/git/ref/heads/main': { object: { sha: oldHead } },
    [prRoute]: [],
    '/repos/owner/project/issues/7/comments': [{ user: owner, html_url: sourceUrl, body: 'Please preserve work.' }],
  };
  const event = { action: 'labeled', repository: repo, sender: owner, issue, label: { name: 'autofix' } };
  const plan = await planForEvent(fakeApi(routes), 'issues', event, bot);
  assert.equal(plan.stage, 'requirements');
  assert.equal(plan.branch, 'codex/autofix/issue-7');
  assert.equal(plan.prNumber, null);
  assert.equal(plan.history[0].url, issue.html_url);
  assert.equal(plan.history[1].role, 'owner');
  routes[prRoute] = [reqPR];
  assert.equal(await planForEvent(fakeApi(routes), 'issues', event, bot), null);
});
