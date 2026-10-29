import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  isOwner,
  ownerLinks,
  verifyPaths,
  planForEvent,
  approvedOnHead,
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
  head: { ref: 'codex/autofix/issue-7/requirements', sha: head, repo: { id: repo.id } },
  base: { ref: 'main' },
  body: '<!-- habeas-autofix {"stage":"requirements","issue":7} -->',
};

function fakeApi(routes) {
  return async path => {
    const key = path.replace(/([?&])per_page=100&page=\d+$/, '');
    if (key === `/repos/owner/project/git/commits/${head}` && !(key in routes)) {
      return { tree: { sha: oldHead } };
    }
    if (key === `/repos/owner/project/git/trees/${oldHead}?recursive=1` && !(key in routes)) {
      return { truncated: false, tree: [{ path: 'md/requirements.md', type: 'blob', mode: '100644', sha: oldHead }] };
    }
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
  assert.equal(plan.requirementsPr, 12);
});

test('owner PR comment is selected without reading other comments', async () => {
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
