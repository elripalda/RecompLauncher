// Sends game problems to the game's developer and closes them here. Runs from issue-triage.yml.
//
// - "Crash: <game> closed with code ..." issues (the crash window's Report on GitHub) and bug forms
//   that say "A game crashes or runs badly" are closed with a link to the game's own issue page.
// - The "not a launcher bug" label does the same for any issue someone on the team marks.
// - Kept open (labelled "needs a look"): the game can't be found in the catalog, or what the player
//   wrote points at the launcher (Easy Setup, installing, updating).
// - Issues the team opened themselves are never closed on their own.
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
    return `Thanks for the report! ${what}, so ${app.name}'s developer is the one who can fix it. Report it to them here (the details are filled in): ${link}\n\nIn the launcher, **Report to the Developer** in the crash window does the same. If you think it's a launcher problem after all, reply here and we'll reopen it.`;
  }
  return `Thanks for the report! ${what}, so its developer is the one who can fix it. Open the game's page in the launcher and use **Report to the Developer**, or report it on the game's GitHub page.\n\nIf you think it's a launcher problem after all, reply here and we'll reopen it.`;
}

async function handle(github, repo, issue, apps, opts, core) {
  if (issue.pull_request || issue.state !== 'open') return;
  const labels = (issue.labels || []).map((l) => (typeof l === 'string' ? l : l.name));
  if (labels.includes(L_LOOK) && !opts.marked) return;
  const kind = kindOf(issue);
  if (!opts.marked && !kind) return;
  if (!opts.marked && TEAM.includes(issue.author_association)) return;
  const app = findGame(issue, apps);
  const note = (String(issue.body || '').match(/\*\*What happened:\*\*([^\n]*)/) || [])[1] || formValue(issue.body, 'What happened\\?');
  if (!opts.marked && (!app || OURS.test(note))) {
    if (!labels.includes(L_LOOK)) await github.rest.issues.addLabels({ ...repo, issue_number: issue.number, labels: [L_LOOK] });
    core.info(`#${issue.number}: kept open (${app ? 'the note points at the launcher' : 'game not found'})`);
    return;
  }
  const link = app ? issueLink(app, issue) : '';
  await github.rest.issues.createComment({ ...repo, issue_number: issue.number, body: message(app, link, kind === 'crash') });
  const add = kind === 'crash' ? [L_CRASH] : (labels.includes(L_NOT) ? [] : [L_NOT]);
  if (add.length) await github.rest.issues.addLabels({ ...repo, issue_number: issue.number, labels: add });
  await github.rest.issues.update({ ...repo, issue_number: issue.number, state: 'closed', state_reason: 'not_planned' });
  core.info(`#${issue.number}: sent to ${app ? app.name : 'the game\'s page'} and closed`);
}

module.exports = async ({ github, context, core }) => {
  const repo = context.repo;
  await ensureLabels(github, repo);
  const apps = await catalog(core);
  const ev = context.eventName;
  if (ev === 'issues') {
    const issue = context.payload.issue;
    if (context.payload.action === 'labeled') {
      if (context.payload.label && context.payload.label.name === L_NOT) await handle(github, repo, issue, apps, { marked: true }, core);
      return;
    }
    await handle(github, repo, issue, apps, { marked: false }, core);
    return;
  }
  // By hand: every open crash or game issue.
  const open = await github.paginate(github.rest.issues.listForRepo, { ...repo, state: 'open', per_page: 100 });
  for (const issue of open) {
    try { await handle(github, repo, issue, apps, { marked: false }, core); } catch (e) { core.warning(`#${issue.number}: ${e.message}`); }
  }
};
