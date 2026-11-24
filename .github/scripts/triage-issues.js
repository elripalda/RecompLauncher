// Sends game problems to the game's developer and closes them here. Runs from issue-triage.yml.
//
// - "Crash: <game> closed with code ..." issues (the crash window's Report on GitHub) and bug forms
//   that say "A game crashes or runs badly" are closed with a link to the game's own issue page.
// - The "not a launcher bug" label does the same for any issue someone on the team marks.
// - Kept open (labelled "needs a look"): the game can't be found in the catalog, or what the player
//   wrote points at the launcher (Easy Setup, installing, updating), or the game's log says the
//   launcher's setup step never built it.
// - Issues the team opened themselves are never closed on their own.
// - Before linking to the game's GitHub, it checks the repo is still there and takes reports. If it's
//   gone (deleted or made private), archived, or has issues turned off, the comment says so plainly
//   instead of posting a dead link. Running it by hand later rewrites those comments, so once a repo
//   comes back the link is added.
// Run by hand (workflow_dispatch), it goes through every open crash issue.

const CATALOG = 'https://recomplauncher.com/pc-catalog.json';
const L_CRASH = 'game crash';
const L_NOT = 'not a launcher bug';
const L_LOOK = 'needs a look';
const TEAM = ['OWNER', 'MEMBER', 'COLLABORATOR'];
const LABELS = {
  [L_CRASH]: ['d93f0b', 'A game crashed: its developer can fix it, not the launcher'],
  [L_NOT]: ['cfd3d7', 'About a game, not the launcher: the bot sends it to the game\'s page'],
  [L_LOOK]: ['fbca04', 'Might be the launcher: someone should read it'],
};
// Words in the player's own note that point at the launcher rather than the game.
const OURS = /easy setup|setting (it )?up|set ?up|install|download|updat|the launcher|recomplauncher|won'?t (start|open|launch) from/i;
// Lines in the game's log that mean the launcher's setup step didn't finish (a game built from the
// player's copy that was never built): that's ours to look at.
const SETUP_LOG = /not built yet|build it with|run (its|the) setup|set up your game copy/i;

async function catalog(core) {
  try {
    const r = await fetch(CATALOG, { headers: { 'User-Agent': 'RecompLauncher-issue-triage' } });
    if (!r.ok) { core.warning(`Catalog answered ${r.status}`); return []; }
    const j = await r.json();
    return Array.isArray(j.apps) ? j.apps : [];
  } catch (e) {
    core.warning(`Catalog: ${e.message}`);
    return [];
  }
}

// The same links the launcher uses (ui/app.js repoIssues), so the player lands where Report to the
// Developer would have sent them.
function issueLink(app, issue) {
  if (/^https:\/\//i.test(String(app.report_url || ''))) return app.report_url;
  const repo = String(app.repo || '');
  if (/^sourceforge:/i.test(repo)) return `https://sourceforge.net/projects/${repo.slice(12)}/support`;
  if (/^gitlab:/i.test(repo)) return `https://gitlab.com/${repo.slice(7)}/-/issues/new`;
  if (/^forgejo:/i.test(repo)) return `https://${repo.slice(8)}/issues`;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) return '';
  const title = encodeURIComponent(issue.title || '');
  const full = `https://github.com/${repo}/issues/new?title=${title}&body=${encodeURIComponent(String(issue.body || '').slice(0, 6000))}`;
  return full.length <= 7500 ? full : `https://github.com/${repo}/issues/new?title=${title}`;
}

// The GitHub repo a report would go to: the catalog's owner/name, or the one in a github.com
// report_url. Empty for other sites (GitLab, SourceForge, Forgejo, a forum), which aren't checked.
function githubRepo(app) {
  const url = String(app.report_url || '');
  if (/^https:\/\//i.test(url)) {
    const m = url.match(/^https:\/\/github\.com\/([\w.-]+)\/([\w.-]+)/i);
    return m ? `${m[1]}/${m[2]}` : '';
  }
  const repo = String(app.repo || '');
  return /^[\w.-]+\/[\w.-]+$/.test(repo) ? repo : '';
}

// Whether the game's GitHub can still take a report: 'ok', 'gone', 'archived', 'no-issues', or
// 'unknown' when GitHub couldn't say (rate limit, outage, network). Unknown posts the link as
// before: a check that didn't get an answer shouldn't hold back a link that most likely works.
// Answers are kept for the run, so a sweep over many issues asks once per repo.
async function repoState(github, full, cache, core) {
  if (!full) return 'unknown';
  const key = full.toLowerCase();
  if (cache.has(key)) return cache.get(key);
  const [owner, repo] = full.split('/');
  let state = 'unknown';
  try {
    // A renamed or moved repo answers with a redirect, which is followed, so it counts as there.
    const { data } = await github.rest.repos.get({ owner, repo });
    if (data.archived) state = 'archived';
    else if (data.has_issues === false) state = 'no-issues';
    else state = 'ok';
  } catch (e) {
    // 404: deleted or made private. 451: taken down. Anything else is GitHub not answering.
    if (e.status === 404 || e.status === 451) state = 'gone';
    else core.warning(`Couldn't check ${full} (${e.status || e.message}), so the link is posted as usual`);
  }
  cache.set(key, state);
  return state;
}

function formValue(body, label) {
  const m = String(body || '').match(new RegExp(`^### ${label}\\s*\\n+([^\\n#][^\\n]*)`, 'mi'));
  const v = m ? m[1].trim() : '';
  return v === '_No response_' ? '' : v;
}

function findGame(issue, apps) {
  const body = String(issue.body || '');
  const byId = body.match(/^- Game: .*\(([a-z0-9][a-z0-9_-]*)\)\s*$/mi);
  if (byId) {
    const a = apps.find((x) => x.id === byId[1]);
    if (a) return a;
  }
  const fromTitle = String(issue.title || '').match(/^Crash:\s*(.+?)\s+closed with code/i);
  const name = (fromTitle ? fromTitle[1] : formValue(body, 'Which game\\?')).trim().toLowerCase();
  if (!name) return null;
  const same = (x) => [x.name, x.game, x.id].some((v) => String(v || '').trim().toLowerCase() === name);
  return apps.find(same) || null;
}

function kindOf(issue) {
  if (/^Crash:/i.test(issue.title || '')) return 'crash';
  if (/A game crashes or runs badly/i.test(formValue(issue.body, "What's it about\\?"))) return 'game';
  return '';
}

async function ensureLabels(github, repo) {
  for (const [name, [color, description]] of Object.entries(LABELS)) {
    try { await github.rest.issues.getLabel({ ...repo, name }); } catch (e) {
      if (e.status === 404) await github.rest.issues.createLabel({ ...repo, name, color, description });
    }
  }
}

function message(app, link, crash) {
  const what = crash ? 'This crash happened in the game itself' : 'This is about the game itself, not the launcher';
  if (app && link) {
    return `Thanks for the report! ${what}, so ${app.name}'s developer is the one who can fix it. **[Report it to them here](${link})** (the details are filled in).\n\nIn the launcher, **Report to the Developer** in the crash window does the same. If you think it's a launcher problem after all, reply here and we'll reopen it.`;
  }
  return `Thanks for the report! ${what}, so its developer is the one who can fix it. Open the game's page in the launcher and use **Report to the Developer**, or report it on the game's GitHub page.\n\nIf you think it's a launcher problem after all, reply here and we'll reopen it.`;
}

// When the game's GitHub can't take the report, say so instead of linking to it. Nothing is
// suggested in the launcher either, since its Report to the Developer opens the same page.
const NO_REPORTS = {
  gone: (name, url) => `But ${name}'s GitHub page (${url}) is gone: it was deleted or made private, so there's nowhere to report it right now. There's nothing we can do until the developer brings it back.`,
  archived: (name, url) => `But ${name}'s GitHub page (${url}) is archived, so it doesn't take new reports. There's nothing we can do unless the developer opens it again.`,
  'no-issues': (name, url) => `But ${name}'s developer has turned off reports on its GitHub page (${url}), so there's nowhere to send it there right now. There's nothing we can do until they turn them back on.`,
};

function noReportMessage(app, full, state, crash) {
  const what = crash ? 'This crash happened in the game itself' : 'This is about the game itself, not the launcher';
  const why = NO_REPORTS[state](app.name, `https://github.com/${full}`);
  return `Thanks for the report! ${what}, so ${app.name}'s developer is the one who can fix it. ${why}\n\nIf you think it's a launcher problem after all, reply here and we'll reopen it.`;
}

// The comment for a game issue: the link to the game's issue page, or why there isn't one.
async function reply(github, app, issue, crash, cache, core) {
  if (!app) return message(null, '', crash);
  const full = githubRepo(app);
  const state = await repoState(github, full, cache, core);
  if (NO_REPORTS[state]) return noReportMessage(app, full, state, crash);
  return message(app, issueLink(app, issue), crash);
}

async function handle(github, repo, issue, apps, opts, core, cache) {
  if (issue.pull_request || issue.state !== 'open') return;
  const labels = (issue.labels || []).map((l) => (typeof l === 'string' ? l : l.name));
  if (labels.includes(L_LOOK) && !opts.marked) return;
  const kind = kindOf(issue);
  if (!opts.marked && !kind) return;
  if (!opts.marked && TEAM.includes(issue.author_association)) return;
  const app = findGame(issue, apps);
  const note = (String(issue.body || '').match(/\*\*What happened:\*\*([^\n]*)/) || [])[1] || formValue(issue.body, 'What happened\\?');
  const setupLog = SETUP_LOG.test(String(issue.body || ''));
  if (!opts.marked && (!app || OURS.test(note) || setupLog)) {
    if (!labels.includes(L_LOOK)) await github.rest.issues.addLabels({ ...repo, issue_number: issue.number, labels: [L_LOOK] });
    core.info(`#${issue.number}: kept open (${!app ? 'game not found' : setupLog ? 'setup didn\'t finish' : 'the note points at the launcher'})`);
    return;
  }
  const body = await reply(github, app, issue, kind === 'crash', cache, core);
  await github.rest.issues.createComment({ ...repo, issue_number: issue.number, body });
  const add = kind === 'crash' ? [L_CRASH] : (labels.includes(L_NOT) ? [] : [L_NOT]);
  if (add.length) await github.rest.issues.addLabels({ ...repo, issue_number: issue.number, labels: add });
  await github.rest.issues.update({ ...repo, issue_number: issue.number, state: 'closed', state_reason: 'not_planned' });
  core.info(`#${issue.number}: sent to ${app ? app.name : 'the game\'s page'} and closed`);
}

module.exports = async ({ github, context, core }) => {
  const repo = context.repo;
  await ensureLabels(github, repo);
  const apps = await catalog(core);
  const cache = new Map();
  const ev = context.eventName;
  if (ev === 'issues') {
    const issue = context.payload.issue;
    if (context.payload.action === 'labeled') {
      if (context.payload.label && context.payload.label.name === L_NOT) await handle(github, repo, issue, apps, { marked: true }, core, cache);
      return;
    }
    await handle(github, repo, issue, apps, { marked: false }, core, cache);
    return;
  }
  // By hand: every open crash or game issue.
  const open = await github.paginate(github.rest.issues.listForRepo, { ...repo, state: 'open', per_page: 100 });
  for (const issue of open) {
    try { await handle(github, repo, issue, apps, { marked: false }, core, cache); } catch (e) { core.warning(`#${issue.number}: ${e.message}`); }
  }
  // And the bot's own comments on ones it closed are brought back to the current wording.
  const closed = await github.paginate(github.rest.issues.listForRepo, { ...repo, state: 'closed', labels: L_CRASH, per_page: 100 });
  for (const issue of closed) {
    try { await reword(github, repo, issue, apps, core, cache); } catch (e) { core.warning(`#${issue.number}: ${e.message}`); }
  }
};

async function reword(github, repo, issue, apps, core, cache) {
  const comments = await github.paginate(github.rest.issues.listComments, { ...repo, issue_number: issue.number, per_page: 100 });
  const mine = comments.filter((c) => c.user && c.user.login === 'github-actions[bot]').pop();
  if (!mine) return;
  const app = findGame(issue, apps);
  // GitHub didn't answer for this repo: keep what the comment says rather than swap a "gone" note
  // back to a link (or the other way) on a guess.
  const full = app ? githubRepo(app) : '';
  if (full && await repoState(github, full, cache, core) === 'unknown') return;
  const body = await reply(github, app, issue, kindOf(issue) === 'crash', cache, core);
  if (mine.body === body) return;
  await github.rest.issues.updateComment({ ...repo, comment_id: mine.id, body });
  core.info(`#${issue.number}: comment reworded`);
}
