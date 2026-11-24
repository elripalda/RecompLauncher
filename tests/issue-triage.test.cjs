const { test } = require('node:test');
const assert = require('node:assert/strict');
const triage = require('../.github/scripts/triage-issues.js');

const AKIBOT = { id: 'akibot', name: 'AkiBot', repo: 'Razoc/Akibot' };
const CRASH = {
  number: 522,
  state: 'open',
  title: 'Crash: AkiBot closed with code 321',
  body: '**What happened:** \n\n- Game: AkiBot v0.3.6 (akibot)\n- Closed with code 321 after 319 s',
  labels: [],
  author_association: 'NONE',
};

// A stand-in for the GitHub client: repos.get answers with `repoAnswer` (data, or an error to throw)
// and everything the bot does to the issue is written down.
function fakeGitHub(repoAnswer) {
  const done = { comments: [], closed: false, repoCalls: 0 };
  const github = {
    rest: {
      issues: {
        getLabel: async () => ({}),
        createLabel: async () => ({}),
        addLabels: async () => ({}),
        createComment: async ({ body }) => { done.comments.push(body); },
        update: async ({ state }) => { done.closed = state === 'closed'; },
      },
      repos: {
        get: async () => {
          done.repoCalls += 1;
          if (repoAnswer instanceof Error) throw repoAnswer;
          return { data: repoAnswer };
        },
      },
    },
    paginate: async () => [],
  };
  return { github, done };
}

function httpError(status) {
  const e = new Error(`HTTP ${status}`);
  e.status = status;
  return e;
}

async function run(github, apps = [AKIBOT]) {
  const saved = global.fetch;
  global.fetch = async () => ({ ok: true, json: async () => ({ apps }) });
  const warnings = [];
  const core = { info() {}, warning: (w) => warnings.push(w) };
  const context = { repo: { owner: 'elripalda', repo: 'RecompLauncher' }, eventName: 'issues', payload: { action: 'opened', issue: CRASH } };
  try {
    await triage({ github, context, core });
  } finally {
    global.fetch = saved;
  }
  return warnings;
}

test('a game whose GitHub is there gets the ready-made report link', async () => {
  const { github, done } = fakeGitHub({ full_name: 'Razoc/Akibot', archived: false, has_issues: true });
  await run(github);
  assert.match(done.comments[0], /Report it to them here\]\(https:\/\/github\.com\/Razoc\/Akibot\/issues\/new\?title=/);
  assert.equal(done.closed, true);
});

test('a game whose GitHub is gone gets told so, with no dead link', async () => {
  const { github, done } = fakeGitHub(httpError(404));
  await run(github);
  assert.match(done.comments[0], /AkiBot's GitHub page \(https:\/\/github\.com\/Razoc\/Akibot\) is gone/);
  assert.match(done.comments[0], /nothing we can do until the developer brings it back/);
  assert.doesNotMatch(done.comments[0], /issues\/new/);
  assert.equal(done.closed, true);
});

test('an archived GitHub is said to take no new reports', async () => {
  const { github, done } = fakeGitHub({ full_name: 'Razoc/Akibot', archived: true, has_issues: true });
  await run(github);
  assert.match(done.comments[0], /is archived, so it doesn't take new reports/);
  assert.doesNotMatch(done.comments[0], /issues\/new/);
});

test('a GitHub with issues turned off is said to take no reports', async () => {
  const { github, done } = fakeGitHub({ full_name: 'Razoc/Akibot', archived: false, has_issues: false });
  await run(github);
  assert.match(done.comments[0], /turned off reports on its GitHub page/);
  assert.doesNotMatch(done.comments[0], /issues\/new/);
});

test('when GitHub is rate limited the link is posted as before', async () => {
  const { github, done } = fakeGitHub(httpError(403));
  const warnings = await run(github);
  assert.match(done.comments[0], /Report it to them here\]\(https:\/\/github\.com\/Razoc\/Akibot\/issues\/new/);
  assert.equal(warnings.length, 1);
});

test('when GitHub can not be reached the link is posted as before', async () => {
  const { github, done } = fakeGitHub(new Error('getaddrinfo ENOTFOUND api.github.com'));
  await run(github);
  assert.match(done.comments[0], /Report it to them here\]/);
});

test('games reported somewhere other than GitHub are not checked', async () => {
  const { github, done } = fakeGitHub(httpError(404));
  await run(github, [{ ...AKIBOT, repo: 'gitlab:razoc/akibot' }]);
  assert.equal(done.repoCalls, 0);
  assert.match(done.comments[0], /https:\/\/gitlab\.com\/razoc\/akibot\/-\/issues\/new/);
});

test('a github.com report_url is checked against its repo', async () => {
  const { github, done } = fakeGitHub(httpError(404));
  await run(github, [{ ...AKIBOT, repo: 'Someone/Else', report_url: 'https://github.com/Razoc/Akibot/issues' }]);
  assert.match(done.comments[0], /\(https:\/\/github\.com\/Razoc\/Akibot\) is gone/);
});

// The by-hand sweep rewrites the bot's comments on closed crash issues to the current wording.
async function sweep(github, oldComment) {
  const closed = { ...CRASH, state: 'closed' };
  github.rest.issues.listForRepo = async () => {};
  github.rest.issues.listComments = async () => {};
  github.rest.issues.updateComment = async ({ body }) => { github.updated = body; };
  github.paginate = async (method, args) => {
    if (method === github.rest.issues.listComments) return [{ id: 1, user: { login: 'github-actions[bot]' }, body: oldComment }];
    return args.state === 'closed' ? [closed] : [];
  };
  const saved = global.fetch;
  global.fetch = async () => ({ ok: true, json: async () => ({ apps: [AKIBOT] }) });
  const context = { repo: { owner: 'elripalda', repo: 'RecompLauncher' }, eventName: 'workflow_dispatch', payload: {} };
  try {
    await triage({ github, context, core: { info() {}, warning() {} } });
  } finally {
    global.fetch = saved;
  }
}

test('the sweep swaps an old link for the "gone" note once the repo is gone', async () => {
  const { github } = fakeGitHub(httpError(404));
  await sweep(github, 'old comment with a link');
  assert.match(github.updated, /is gone/);
});

test('the sweep leaves comments alone when GitHub does not answer', async () => {
  const { github } = fakeGitHub(httpError(503));
  await sweep(github, 'AkiBot is gone note');
  assert.equal(github.updated, undefined);
});
