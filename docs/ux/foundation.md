<!-- Managed with super-ux (ux-contract v2). The WHY layer: update when the understanding of users changes. -->

# UX Foundation — The WHY Layer

**Personas → Jobs to Be Done → Customer journeys → User stories.** Scenarios
in [scenarios.md](scenarios.md) trace up to these IDs; audits and coverage
checks key off the chain.

> **Provenance.** This layer was reverse-engineered from the 44 shipped
> scenarios (no interviews, analytics, or support tickets were available).
> Every capability is **observed** (traceable to implemented UI). Every
> motivation, emotion score, and pain is **inferred** — a reconstruction of
> the job the current UI implicitly serves, to be confirmed with a real user.
> Inferred rows carry `(inferred)`; unvalidated bets are marked as
> assumptions in [§5](#5-assumptions--open-questions).
>
> **Validation 2026-07-22.** The operator reviewed the layer against their
> stated product vision. JTBD-01..09 confirmed (nothing contradicted).
> The vision added two new jobs — **JTBD-10 (autonomous continuity: CEO
> supervisor agent, post-task workflow, keep-awake)** and **JTBD-11
> (operations analytics)** — plus task priority (ST-037). Entries sourced
> from that session carry `(stated 2026-07-22)` — operator-stated, the
> strongest provenance in this file. Assumption A-1 was confirmed as a real
> pain and folds into JTBD-10.

---

## 1. Personas

Builder Pro AI serves essentially **one operator in three lifecycle states**.
The three persona IDs below are those states, not three different people — they
are kept distinct because scenarios legitimately branch on them (cold start vs.
steady state vs. post-restart).

### P-01: Solo Builder *(primary — the steady state, was "owner")*
A solo developer / indie hacker running 5–6 software projects at once, each
driven by AI coding agents (Claude Code and the like) inside terminal
sessions. Deeply technical, lives in the terminal, context-switches all day.
Opens the app to answer **"where is each project, what moved, what needs me"**
in under 30 seconds, then dives into the one session that is blocked. Values
honesty over reassurance — would rather see a red banner than a fake "all
good".

### P-02: First-run Builder *(cold start, was "new-user")*
The same operator on first launch: no workspaces, no projects, no daemon
history. Impatient, skims rather than reads. Wants to reach **a working
terminal plus one captured idea** with zero onboarding text. Judges the tool
in the first minute.

### P-03: Returning Builder *(post-restart, was "returning-owner")*
The same operator reopening the app — or the app reconnecting after a
background-daemon restart — with live sessions, projects, and scrollback that
**must reappear intact**. One bad restart that loses an agent's context breaks
trust permanently.

---

## 2. Jobs to Be Done

### JTBD-01: Triage attention across many parallel projects
- **Statement:** When I sit down after being away from my machine, I want to see which of my running agents is blocked waiting on me and which is still working, so I can unblock the ones that need me first without hunting through terminal tabs.
- **Personas:** P-01, P-03
- **Type:** functional
- **Forces:** push: agents stall silently and I don't notice for an hour; pull: one screen that says "these two need you"; anxiety: does the summary actually reflect live state or is it stale; habit: alt-tabbing through every terminal to check by hand.
- **Success metric:** time from opening the app to acting on the first blocked session (< 30s).

### JTBD-02: Run many agent sessions in parallel without losing any
- **Statement:** When I'm driving several coding agents at once, I want each in its own persistent terminal that survives tab switches and app/daemon restarts, so I never lose an agent's mid-task context or scrollback.
- **Personas:** P-01, P-03
- **Type:** functional
- **Forces:** push: a lost PTY means re-explaining the whole task to the agent; pull: tabs that keep buffering when hidden and rehydrate after a restart; anxiety: did switching away kill the session; habit: separate native terminal windows outside the app.
- **Success metric:** zero lost sessions across a tab switch or a daemon restart; scrollback intact after rehydrate.

### JTBD-03: Inspect what the agents are doing to the filesystem
- **Statement:** When an agent edits files in a workspace, I want to browse and preview the changes in place without leaving the app, so I can verify the work without breaking context.
- **Personas:** P-01
- **Type:** functional
- **Forces:** push: switching to a separate editor loses my place; pull: a live file tree beside the terminal; anxiety: is the tree showing the real current state or a stale cache; habit: `ls`/`cat` in another shell.
- **Success metric:** the file the agent just wrote is visible and previewable within a couple of clicks, and the tree is trustably current.

### JTBD-04: Capture an idea the instant it strikes
- **Statement:** When an idea hits me mid-work, I want to capture it in one keystroke without leaving what I'm doing, so it isn't lost and I can triage it later.
- **Personas:** P-01, P-02
- **Type:** functional
- **Forces:** push: ideas evaporate if I have to stop and file them; pull: a global ⌘K that never steals focus from the terminal; anxiety: will an unfiled idea just disappear; habit: scratch notes in a random text file.
- **Success metric:** an idea captured in one keystroke reliably reappears later (in a project or the Inbox).

### JTBD-05: Turn a raw idea into researched, prioritized work
- **Statement:** When I have a rough idea, I want to research it, form a grounded insight, and drop the resulting task into a backlog, so the idea becomes actionable work instead of a stale note.
- **Personas:** P-01
- **Type:** functional
- **Forces:** push: ideas pile up unresearched and undecided; pull: a pipeline that carries idea → research → insight → task without re-typing; anxiety: is the research grounded in this project's real context or generic; habit: deciding by gut and forgetting the reasoning.
- **Success metric:** an idea reaches the backlog as a task with its insight/reasoning attached, exactly once (no duplicates on retry).

### JTBD-06: Keep each project's intent and structure in one place
- **Statement:** When I run many projects, I want each to hold its goals, tasks, ideas, insights, rules, and a knowledge graph together, so I always know what a project is for and what's next.
- **Personas:** P-01
- **Type:** functional
- **Forces:** push: strategy scattered across READMEs and my head; pull: one panel per project with goals and a live graph; anxiety: is this structure worth maintaining by hand; habit: keeping it all in memory and losing it on context-switch.
- **Success metric:** for any project, "what's the goal and what's next" is answerable from its panel without leaving the app.

### JTBD-07: Extend agents with external tools — safely
- **Statement:** When I want my agents to use external MCP tools and connectors, I want to connect servers behind explicit consent, spend caps, and honest "unverified" labeling, so I gain capability without losing control or trusting output blindly.
- **Personas:** P-01
- **Type:** functional + emotional (control/trust)
- **Forces:** push: powerful tools are risky to wire up blindly; pull: a consent gate, spend/rate caps, and an audit log; anxiety: will a tool spend without limit or hand me fabricated data as fact; habit: running tools directly in the shell with no guardrails.
- **Success metric:** no tool connects or spends without an explicit gate; every tool result is labeled unverified; spend stays under the set cap.

### JTBD-08: Trust the app through background-service failure
- **Statement:** When a background daemon drops, needs upgrading, or the database degrades, I want honest banners and self-healing reconnects instead of silent data loss or fake success, so I always know the true state and never lose work.
- **Personas:** P-01, P-03
- **Type:** emotional (trust)
- **Forces:** push: a tool that lies about its state once is untrustworthy forever; pull: red banners that name the real problem and clear only when truly fixed; anxiety: is my data actually persisted right now; habit: restarting everything and hoping.
- **Success metric:** the operator can always tell whether the app is healthy, degraded, or in-memory — and never discovers data loss after the fact.

### JTBD-09: Get from cold start to driving agents, fast
- **Statement:** When I first open the app or add a new project folder, I want to reach a working terminal and a linked workspace with zero reading, so I can start driving agents immediately.
- **Personas:** P-02
- **Type:** functional
- **Forces:** push: setup friction makes me abandon a new tool; pull: an empty state that tells me the one next action; anxiety: do I have to configure a lot before it's useful; habit: sticking with my existing terminal setup.
- **Success metric:** time from first launch to a live terminal in a real workspace (< 2 min, no docs).

### JTBD-10: Keep work flowing without me *(stated 2026-07-22)*
- **Statement:** When I step away while agents are mid-work, I want a supervisor (CEO agent) to answer their questions and hand them the next task per the project workflow — within the authority I delegated — while the machine stays awake, so work continues instead of stalling until I return.
- **Personas:** P-01, P-03
- **Type:** functional + emotional (control/trust)
- **Forces:** push: agents stall on a question for hours while I'm away, and macOS sleep kills long runs; pull: delegated decisions grounded in project rules/policy (spend caps, confirmation classes, allowed paths — the substrate SCN-036 already ships) plus automatic workflow continuation; anxiety: a wrong autonomous decision does damage — every decision must be capped, classed, and logged; habit: babysitting every terminal and answering everything myself.
- **Success metric:** share of agent questions resolved within policy without the operator; zero stalled sessions found on return; 100% of autonomous decisions logged with reasoning and reviewable.

### JTBD-11: Understand my own operation *(stated 2026-07-22)*
- **Statement:** When I run many agents across many projects, I want statistics on agent usage, tokens, costs, commits, and code output, so I can see where effort and money go and steer accordingly.
- **Personas:** P-01
- **Type:** functional
- **Forces:** push: flying blind on spend and output across 5–6 projects; pull: one in-app dashboard with per-project/per-agent cuts and time ranges; anxiety: stats that lie or cost more to collect than they're worth; habit: guessing, or scraping logs and git by hand.
- **Success metric:** "where did this week's tokens/money/commits go, per project" answerable in-app in under a minute.

---

## 3. Customer journeys

Emotion 1 (frustrated) – 5 (delighted). Pain/Opportunity are **(inferred)**.
Opportunity priority = Frequency × Severity × Solvability where scored.

### JRN-01: First-run Builder — get set up fast (JTBD-09)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Before | Installs, expects yet-another-tool friction | download/install | 3 | new tools usually demand config first | — |
| 2 | First open | Sees empty Home + one-sentence guide | Home empty state, sidebar CTAs (SCN-001) | 4 | nothing yet to act on; must trust the CTA | lean, single-next-action empty state (BP-003) |
| 3 | Setup | Adds a workspace folder | "+ Add workspace" → OS picker (SCN-002) | 4 | folder-picker is OS-native, unbranded | drop straight into the workspace view (done) |
| 4 | First value | Opens a terminal, drives an agent | "+ New terminal" (SCN-013) | 5 | none — this is the aha | make "reach first terminal" the tracked activation event (BP-040/041) |
| 5 | After | Captures a first idea, keeps working | ⌘K (SCN-003) | 4 | must discover ⌘K exists | surface the hotkey once in the empty state (assumption A-3) |

### JRN-02: Solo Builder — triage what needs me (JTBD-01)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Before | Returns after time away; agents ran unattended | — | 2 | no idea which agent stalled | — |
| 2 | Scan | Reads "Needs you / Running / Recently finished" | Home groups + stat tiles (SCN-004 → SCN-055) | 4 | the agent's QUESTION is invisible — must open each session to judge priority | question preview + escalation reason on the row (SCN-055) |
| 3 | Confirm | Reads status dots per session | StatusDot (SCN-016) | 4 | a stale event could mislead | "exited always wins" guarantee (done) |
| 4 | Act | Clicks "Go →" on a blocked row | Home row → workspace + focus terminal (SCN-004 → SCN-055) | 5 | none | one-click jump lands focus in the PTY (done) |
| 5 | After | Unblocks agent, returns to Home for the next | "since you left" digest + needs-you ranking (SCN-055) | 4 | must re-scan manually | digest + ranked escalations resolve A-1 fully (SCN-055, with SCN-048 badge) |

### JRN-03: Solo Builder — run agents in parallel (JTBD-02)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Open | Spawns terminals per task | "+ New terminal" (SCN-013) | 4 | cwd must land in the right root | cwd defaults to selected file's root (done) |
| 2 | Switch | Flips between session tabs | tab strip keep-alive (SCN-014) | 5 | fear a hidden tab died | hidden tabs keep buffering, no re-spawn (done) |
| 3 | Monitor | Glances at command outcomes | CommandStrip OSC-133 (SCN-017) | 4 | needs shell integration to emit events | ✓/✗/running/interrupted chips (done) |
| 4 | Navigate | Clicks a path/URL in output | link provider (SCN-018) | 4 | out-of-root paths are ambiguous | honest "outside workspace" toast (done) |
| 5 | Close | Kills a finished session | tab × (SCN-015) | 4 | a failed kill could zombie a tab | tab removed even if kill rejects (done) |

### JRN-04: Returning Builder — survive a restart (JTBD-02 / JTBD-08)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Trigger | App reopens / daemon restarts | daemon reconnect (SCN-019) | 2 | dread of lost sessions | — |
| 2 | Wait | Sees a disconnect banner, no action asked | DaemonBanner (SCN-037) | 3 | is it hung or recovering | self-healing banner, backoff retries (done) |
| 3 | Rehydrate | Sessions reappear as tabs | rehydrate + fresh replay (SCN-019) | 5 | duplicated scrollback would confuse | term.reset before replay — no dupes (done) |
| 4 | Edge | An attach fails mid-rehydrate | pane overlay + Retry (SCN-044) | 3 | a silent failure would look like data loss | per-session error surfaced + Retry (done) |
| 5 | After | Resumes work, trust reinforced | — | 4 | — | restart is the trust-defining moment (assumption A-2) |

### JRN-05: Solo Builder — idea to backlog (JTBD-04 + JTBD-05)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Capture | ⌘K, types idea, maybe picks a project | QuickCapture (SCN-003) | 4 | orphan ideas could get lost | orphans routed to Inbox, not dropped (SCN-028) |
| 2 | Rescue | Later links/promotes orphan ideas | Inbox panel (SCN-028) | 4 | partial spawn could duplicate | resumable spawn, no duplicates (done) |
| 3 | Research | Runs an MCP tool on the idea | ResearchRunDialog (SCN-026) | 3 | spend/validity anxiety | spend preflight + self-polling status (done) |
| 4 | Decide | Forms an insight from research | FormInsightDialog (SCN-027) | 4 | is it grounded in this project | fit-context panel: goals + graph (done) |
| 5 | Act | Sends to backlog as a task | "To backlog" (SCN-027, SCN-030) | 5 | retry could double-create | createdTaskId/insightId guard (done) |

### JRN-06: Solo Builder — inspect the workspace (JTBD-03)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Browse | Expands the file tree | FileTree lazy fetch (SCN-020) | 4 | large trees, ignored noise | dirs-first sort, "show ignored" toggle (done) |
| 2 | Preview | Opens a file the agent changed | FilePreview (SCN-021) | 4 | binary/huge files break preview | honest "binary"/"too large" states (done) |
| 3 | Edit | Creates/renames/deletes entries | row menu (SCN-022, SCN-023) | 4 | accidental destructive action | delete → Trash, with confirm (done) |
| 4 | Trust | Watcher dies; tree could go stale | "live updates paused — refresh" (SCN-024) | 3 | a silently stale tree misleads | degradation banner + cache drop on refresh (done) |

### JRN-07: Solo Builder — organize a project (JTBD-06)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Create | Makes a project, links a workspace | CreateProjectDialog (SCN-009) | 4 | forced to pick a workspace up front | inline "+ create workspace" (done) |
| 2 | Structure | Sets goals, tasks, rules | Goals/Tasks/Rules tabs (SCN-031, SCN-030, SCN-036) | 4 | manual upkeep cost | one panel, seeded root goal (done) |
| 3 | Map | Builds a knowledge graph | GraphCanvas (SCN-032) | 3 | graph upkeep is effort-heavy | optimistic edges, entityRef nodes (done); (assumption A-4: is manual graph worth it) |
| 4 | Overview | Scans goals from Home | HomeGoals (SCN-005) | 4 | many projects to scan | per-project goal blocks (done) |
| 5 | Move | Exports / archives a project | export-import, archive (SCN-011, SCN-012) | 4 | fear archive = delete | archive is read-only + reversible (done) |

### JRN-08: Solo Builder — connect an MCP tool safely (JTBD-07)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Add | Adds a server endpoint + auth | ServersTab add form (SCN-033) | 3 | trust the endpoint | bearer/no-auth only; OAuth/stdio marked "soon" (done) |
| 2 | Consent | Confirms a connect gate | ConnectDialog (SCN-033) | 4 | connecting blindly is risky | explicit consent gate + access note (done) |
| 3 | Invoke | Runs a tool, reads result | ToolsBrowser (SCN-034) | 4 | is the output real | unconditional "⚠ unverified data" banner (done) |
| 4 | Govern | Sets spend/rate caps, reviews log | InvocationLog (SCN-035) | 4 | runaway spend | caps + Calls/Audit tables (done) |
| 5 | After | Reuses connected servers in research | research server select (SCN-026) | 4 | — | connected servers flow into the pipeline (done) |

### JRN-09: Solo Builder — a daemon fails (JTBD-08)
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Drop | Orchd/sessiond goes down | OrchdDownBanner / DaemonBanner (SCN-039, SCN-037) | 2 | silent failure would be worst | global red banner, mutating controls disable (done) |
| 2 | Upgrade | An incompatible daemon needs update | UpgradeDialog (SCN-038, SCN-040) | 2 | losing live sessions on update | dialog states what's saved; Cancel leaves a re-entry banner (done) |
| 3 | Degrade | DB runs in-memory / was reset | StorageBanner (SCN-041) | 1 | changes silently not persisting | blunt "will NOT survive a restart" banner (done) |
| 4 | Diagnose | Reviews recorded errors | DiagnosticsPanel (SCN-042) | 3 | a 4s toast forgets the cause | every failure also lands in Diagnostics (done) |
| 5 | Crash | A view crashes during render | ErrorBoundary (SCN-043) | 2 | a white screen = dead app | recovery card + "Reload app" (done) |

### JRN-10: Solo Builder — steps away, work continues (JTBD-10) *(stated 2026-07-22)*
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Delegate | Sets rules, policy caps, confirmation classes; enables the CEO | Rules tab (SCN-036) + supervisor setup (SCN-046, draft) | 4 | defining the trust boundary is hard | policy substrate already shipped; add explicit delegation scope |
| 2 | Away — question | Agent asks; CEO answers within authority | supervisor (SCN-047, draft) | 3 | fear of a wrong autonomous call | class+cap gates, full decision log |
| 3 | Away — escalation | Out-of-authority question parks as "needs you" | escalation surface (SCN-048, draft) | 3 | escalations invisible until Home visit | persistent needs-you signal (A-1 confirmed) |
| 4 | Away — next task | Agent finishes; CEO assigns next per workflow, respecting priority | workflow continuation (SCN-049, draft) | 4 | empty/ambiguous backlog | honest idle state; priority field (ST-037) |
| 5 | Machine | Long run, screen locked — machine must not sleep | keep-awake (SCN-045, draft) | 2 | macOS sleep kills runs silently | sleep assertion + visible indicator |
| 6 | Return | Reviews "while you were away": decisions, progress, escalations | decision log (SCN-050, draft) | 5 | reconstructing what happened from scrollback | digest + reviewable reasoning per decision |

### JRN-11: Solo Builder — reads the operation (JTBD-11) *(stated 2026-07-22)*
| # | Stage | User action | Touchpoint | Emotion | Pain | Opportunity |
|---|-------|------------|------------|---------|------|-------------|
| 1 | Wonder | "Where did this week's tokens/money go?" | — | 2 | no in-app answer today | — |
| 2 | Open | Opens the stats dashboard, picks a range | analytics view (SCN-052, draft) | 4 | — | home for the orphan atoms: SegmentedPill «All\|30d\|7d» + Heatmap activity (COV-01) |
| 3 | Cut | Slices by project/agent: tokens, cost, calls | analytics view (SCN-052, draft) | 4 | data must be trustworthy | reuse InvocationLog cost plumbing (SCN-035) |
| 4 | Output | Reads commits + code delta per project | output stats (SCN-053, draft) | 4 | git scraping cost | derive from workspace git, cache honestly |
| 5 | Act | Spots an outlier → adjusts caps, kills an agent, rebalances | Rules tab / terminals | 5 | — | insight → action in one app |

---

## 4. User stories

INVEST; acceptance criteria Given/When/Then and observable. Status `delivered`
= the scenario is implemented and last audited PASS. Each story lists the
scenario(s) that serve it; every scenario traces back to exactly one story.

### ST-001: Reach a working state on first launch
- **Story:** As P-02, I want the empty app to tell me the single next thing to do, so that I'm not stranded on a blank screen.
- **Traces:** JTBD-09, JRN-01/#2
- **Acceptance criteria:**
  - Given no saved state, when the app opens, then Home shows zeroed stat tiles, an empty-sessions line, and "+ project" / "+ Add workspace" CTAs.
  - Given the daemon isn't connected yet, when Home renders, then a red "Daemon disconnected — reconnecting…" banner shows and auto-retries.
- **Priority:** must
- **Status:** delivered *(SCN-001)*

### ST-002: Add a workspace folder
- **Story:** As P-02, I want to turn a folder into a workspace in two clicks, so that I can start working in it immediately.
- **Traces:** JTBD-09, JRN-01/#3
- **Acceptance criteria:**
  - Given the sidebar, when I click "+ Add workspace" and pick a folder, then a workspace is created from the folder name and the app navigates into it.
  - Given the create fails, when it rejects, then a toast names the reason (disconnected / incompatible / too large / internal).
- **Priority:** must
- **Status:** delivered *(SCN-002; fast-path extension SCN-056 validated)*

### ST-003: Capture an idea without breaking flow
- **Story:** As P-01/P-02, I want a global one-keystroke capture, so that ideas don't interrupt my terminal work.
- **Traces:** JTBD-04, JRN-01/#5, JRN-05/#1
- **Acceptance criteria:**
  - Given any view, when I press ⌘K, then a focused "New idea" dialog opens; Enter saves, and it is ignored while typing in an input/terminal or when an upgrade dialog is open.
  - Given orchd is down, when the dialog is open, then Save is disabled with an "orchestrator unavailable" note.
- **Priority:** must
- **Status:** delivered *(SCN-003)*

### ST-004: Triage what needs me across projects
- **Story:** As P-01, I want a single attention view grouping waiting / running / finished sessions, so that I act on blocked agents first.
- **Traces:** JTBD-01, JRN-02/#2, JRN-02/#4
- **Acceptance criteria:**
  - Given sessions across workspaces, when I open Home, then stat tiles show workspaces/live/waiting (tone changes when > 0) and rows group by state.
  - Given a waiting row, when I click "Go →", then the app navigates to that workspace, activates the session, and focuses its terminal.
- **Priority:** must
- **Status:** delivered *(SCN-004)*

### ST-005: See strategic goal status at a glance
- **Story:** As P-01, I want per-project goal status on Home, so that I keep the strategic picture without opening each project.
- **Traces:** JTBD-06, JRN-07/#4
- **Acceptance criteria:**
  - Given ≥1 active project, when I view Home, then each project shows its strategic goal title and child-goal status chips; clicking a block opens that project.
  - Given goals are still fetching, when all active projects are unfetched, then a "Goals are loading…" line shows.
- **Priority:** should
- **Status:** delivered *(SCN-005)*

### ST-006: Control appearance
- **Story:** As P-01, I want a theme toggle that persists and never flashes, so that the app matches my environment.
- **Traces:** JTBD-06
- **Acceptance criteria:**
  - Given the toggle, when I click it, then the theme cycles system → light → dark → system, persists in localStorage, and applies before first paint.
- **Priority:** could
- **Status:** delivered *(SCN-006)*

### ST-007: Navigate between home, projects, and workspaces
- **Story:** As P-01, I want an always-visible sidebar to move between Home, Extensions, Inbox, projects, and workspaces, so that I orient instantly.
- **Traces:** JTBD-01, JTBD-06
- **Acceptance criteria:**
  - Given the sidebar, when I click a destination, then the view switches; a project header opens its panel; a workspace row activates it.
  - Given an unlinked workspace, when I pick a project in its inline link select, then it attaches and moves under that project group.
- **Priority:** must
- **Status:** delivered *(SCN-007, SCN-008)*

### ST-008: Create and structure a project
- **Story:** As P-01, I want to create a project and manage its workspaces and tabs, so that each project is a real container for work.
- **Traces:** JTBD-06, JRN-07/#1, JRN-07/#2
- **Acceptance criteria:**
  - Given "+ project", when I name it and select/create ≥1 workspace, then it is created and appears in the sidebar; Create is blocked with zero workspaces or an empty name.
  - Given the project panel, when I switch tabs or unlink/attach a workspace, then counters and the workspace list update; orchd-down disables mutations while reads stay live.
- **Priority:** must
- **Status:** delivered *(SCN-009, SCN-010)*

### ST-009: Move and retire projects safely
- **Story:** As P-01, I want to export/import a project and archive it reversibly, so that I can move work between machines and retire it without fear of deletion.
- **Traces:** JTBD-06, JRN-07/#5
- **Acceptance criteria:**
  - Given the Overview tab, when I copy JSON / save to file / import from a folder, then a toast confirms each; an empty folder shows an honest "no .json files" note.
  - Given archive, when I confirm, then the project becomes read-only (export still works) and moves to "Archived (N)"; un-archive is reversible with no confirm.
- **Priority:** should
- **Status:** delivered *(SCN-011, SCN-012)*

### ST-010: Run agents in persistent terminals
- **Story:** As P-01, I want to open, switch, and close terminal sessions that stay alive when hidden, so that I drive many agents without losing any.
- **Traces:** JTBD-02, JRN-03/#1, JRN-03/#2, JRN-03/#5
- **Acceptance criteria:**
  - Given an active workspace, when I click "+ New terminal", then a session spawns with the right cwd and its tab auto-activates.
  - Given 2+ sessions, when I switch tabs, then the pane shows full scrollback with no re-spawn; hidden sessions keep buffering.
  - Given a tab ×, when I close it, then the PTY is killed and the tab removed even if the kill rejects (no zombie tab).
- **Priority:** must
- **Status:** delivered *(SCN-013, SCN-014, SCN-015)*

### ST-011: Know each session's state at a glance
- **Story:** As P-01, I want a status dot per session, so that I can tell running / waiting / idle / exited without opening the terminal.
- **Traces:** JTBD-01, JTBD-02, JRN-02/#3
- **Acceptance criteria:**
  - Given a running session, when its state changes, then the dot reflects running (info) / waiting-for-input (warn) / idle (muted) / exited (danger).
  - Given an exited session, when a late state event arrives, then it cannot resurrect the session (exited wins).
- **Priority:** must
- **Status:** delivered *(SCN-016)*

### ST-012: Review command outcomes
- **Story:** As P-01, I want a strip of recent commands with pass/fail, so that I see what the agent ran without scrolling the buffer.
- **Traces:** JTBD-02, JRN-03/#3
- **Acceptance criteria:**
  - Given OSC-133 shell integration, when commands run, then the last 10 show as chips (✓ / ✗ {code} / running / interrupted).
  - Given the fetch fails, when the strip loads, then it shows an error line with Retry, not a blank.
- **Priority:** should
- **Status:** delivered *(SCN-017)*

### ST-013: Jump from terminal output to files and the browser
- **Story:** As P-01, I want clickable paths and links in terminal output, so that I can open a file or URL without copy-pasting.
- **Traces:** JTBD-03, JRN-03/#4
- **Acceptance criteria:**
  - Given output with a workspace path or OSC-8 link, when I click it, then a workspace file opens in preview and an http(s) link opens the OS browser.
  - Given a path outside the roots, when I click it, then an honest "outside the workspace or not found" toast shows.
- **Priority:** should
- **Status:** delivered *(SCN-018)*

### ST-014: Never lose sessions across a restart
- **Story:** As P-03, I want sessions to rehydrate intact after a daemon or app restart, so that I never lose an agent's context.
- **Traces:** JTBD-02, JTBD-08, JRN-04/#3, JRN-04/#4
- **Acceptance criteria:**
  - Given daemon-side sessions, when the daemon reconnects or the app cold-boots, then sessions reappear as tabs; the visible one re-attaches with a fresh replay (no duplicated scrollback), hidden ones lazily on switch.
  - Given an attach rejects mid-rehydrate, when it fails, then a per-session overlay "Terminal could not attach: {msg}" with Retry shows and clears on the next attempt.
- **Priority:** must
- **Status:** delivered *(SCN-019, SCN-044)*

### ST-015: Inspect the workspace filesystem
- **Story:** As P-01, I want to browse and preview workspace files beside the terminal, so that I can verify agent output in place.
- **Traces:** JTBD-03, JRN-06/#1, JRN-06/#2
- **Acceptance criteria:**
  - Given a workspace with roots, when I expand a directory, then it lazily lists (dirs first, ignored dimmed/toggleable) and a failed read shows an inline Retry.
  - Given a file click, when it loads, then text renders; binary shows "Binary file · {size}", > 1 MiB shows "too large", and a change-under-read shows a truncation banner.
- **Priority:** must
- **Status:** delivered *(SCN-020, SCN-021)*

### ST-016: Manipulate files safely
- **Story:** As P-01, I want to create, rename, and delete files/folders from the tree, so that I can manage the workspace without a separate tool.
- **Traces:** JTBD-03, JRN-06/#3
- **Acceptance criteria:**
  - Given a row menu, when I pick New file/folder or Rename and type a name, then the entry is created/renamed and the parent re-lists; a name clash toasts honestly.
  - Given Delete, when I confirm, then the entry moves to Trash and a previewed-deleted file clears its preview.
- **Priority:** should
- **Status:** delivered *(SCN-022, SCN-023)*

### ST-017: Trust that the file tree is live
- **Story:** As P-01, I want to be told when live file-watching stops, so that I never act on a silently stale tree.
- **Traces:** JTBD-03, JRN-06/#4
- **Acceptance criteria:**
  - Given the watcher dies, when it errors, then an amber "live updates paused — refresh" button appears (and a warn dot on the collapsed rail).
  - Given I click refresh, when the watch restarts, then every cached dir is dropped so the tree re-pulls; a failed restart keeps the paused flag (never falsely "live").
- **Priority:** should
- **Status:** delivered *(SCN-024)*

### ST-018: Manage a project's ideas
- **Story:** As P-01, I want to create, edit, and lifecycle ideas inside a project, so that a project's idea list stays current.
- **Traces:** JTBD-05, JRN-05
- **Acceptance criteria:**
  - Given the Ideas tab, when I add/edit/lifecycle/delete an idea, then the list updates newest-first; empty title disables "+ idea"; a rejected save reverts and toasts.
  - Given orchd is down, when I view the tab, then mutating controls disable while reads stay live.
- **Priority:** must
- **Status:** delivered *(SCN-025)*

### ST-019: Rescue orphan ideas from the Inbox
- **Story:** As P-01, I want an Inbox for ideas captured with no project, so that quick captures don't get lost.
- **Traces:** JTBD-04, JTBD-05, JRN-05/#2
- **Acceptance criteria:**
  - Given orphan ideas, when I open the Inbox (badge shows the count), then I can link one to a project or spawn a project (folder → workspace → project → link).
  - Given a partial spawn failure, when I retry, then it resumes without creating duplicates and the badge count drops on success.
- **Priority:** must
- **Status:** delivered *(SCN-028)*

### ST-020: Research an idea
- **Story:** As P-01, I want to run a connected MCP tool against an idea with a spend preflight, so that I can research it under cost control.
- **Traces:** JTBD-05, JTBD-07, JRN-05/#3
- **Acceptance criteria:**
  - Given a connected server, when I pick a tool, review seeded JSON args and the spend preflight, and click Run, then a run appears and self-polls until terminal; "show artifact" renders a done result.
  - Given invalid JSON or a start failure, when I run, then an inline error shows and the dialog stays open for retry.
- **Priority:** must
- **Status:** delivered *(SCN-026)*

### ST-021: Convert research into a backlog task
- **Story:** As P-01, I want to form a grounded insight from a run and send it to the backlog, so that research becomes actionable work.
- **Traces:** JTBD-05, JRN-05/#4, JRN-05/#5
- **Acceptance criteria:**
  - Given a run, when I review the fit-context (goals + graph) and fill name/verdict/reasoning, then Create → Accept → To backlog progresses and creates a task, moving the idea to "specced".
  - Given an orphan idea, when I open the dialog, then a "context unavailable" note shows and "To backlog" is blocked; retries never double-create.
- **Priority:** must
- **Status:** delivered *(SCN-027)*

### ST-022: Curate insights
- **Story:** As P-01, I want to status and re-verdict insights, so that the insight list reflects my current judgment.
- **Traces:** JTBD-05, JTBD-06
- **Acceptance criteria:**
  - Given the Insights tab, when I set a status, then non-archive applies immediately and archive requires a typed reason; the verdict badge updates.
  - Given orchd is down, when I view the tab, then status/archival/apply disable.
- **Priority:** should
- **Status:** delivered *(SCN-029)*

### ST-023: Plan work as tasks
- **Story:** As P-01, I want a six-status task board per project, so that I track work from backlog to done.
- **Traces:** JTBD-06, JRN-07/#2
- **Acceptance criteria:**
  - Given the Tasks tab, when I add/move/reorder/delete tasks, then six groups always render with counts; reorder is a single fractional-rank call; delete confirms and removes the subtree.
- **Priority:** must
- **Status:** delivered *(SCN-030)*

### ST-024: Steer strategy via a goal tree
- **Story:** As P-01, I want a goal tree with a fixed strategic root, so that a project's direction is explicit and structured.
- **Traces:** JTBD-06, JRN-07/#2
- **Acceptance criteria:**
  - Given the Goals tab, when I add subgoals, rename, status, reorder, or edit metric chips, then the DFS-indented tree updates; the strategic root is never movable or deletable and there is no top-level add.
- **Priority:** should
- **Status:** delivered *(SCN-031)*

### ST-025: Map project knowledge as a graph
- **Story:** As P-01, I want a knowledge graph of nodes and typed edges, so that I can see how a project's concepts, facts, and decisions relate.
- **Traces:** JTBD-06, JRN-07/#3
- **Acceptance criteria:**
  - Given the Graph tab, when I add/drag/connect/rename/retype/delete/search, then the canvas reflects each change; a rejected edge rolls back optimistically; entityRef nodes show "ref · {type}" or "source removed".
- **Priority:** could
- **Status:** delivered *(SCN-032)*

### ST-026: Connect MCP servers with explicit consent
- **Story:** As P-01, I want a consent gate before any MCP server connects, so that I stay in control of what my agents can reach.
- **Traces:** JTBD-07, JRN-08/#1, JRN-08/#2
- **Acceptance criteria:**
  - Given the add form, when I add a server (no-auth or bearer; OAuth/stdio disabled "soon") and click connect, then a ConnectDialog shows the endpoint and access note and connects only on confirm.
  - Given a consent-kind rejection, when it occurs, then the toast tells me how to reconnect via Extensions → Servers → Connect.
- **Priority:** must
- **Status:** delivered *(SCN-033)*

### ST-027: Invoke tools with honest labeling
- **Story:** As P-01, I want tool/connector results marked unverified, so that I never mistake tool output for ground truth.
- **Traces:** JTBD-07, JRN-08/#3
- **Acceptance criteria:**
  - Given a connected server or connector, when I invoke a tool, then results render with an unconditional "⚠ unverified data" banner and errors surface as "the tool returned an error".
  - Given no OAuth providers, when I open Connectors, then an honest "add one in oauth_providers.json" note shows.
- **Priority:** must
- **Status:** delivered *(SCN-034)*

### ST-028: Govern spend and audit tool calls
- **Story:** As P-01, I want spend/rate caps and an audit log, so that tool usage stays within budget and reviewable.
- **Traces:** JTBD-07, JRN-08/#4
- **Acceptance criteria:**
  - Given the Log/Artifacts/Skills tabs, when I set a limit, then it persists; Calls and Audit tables list invocations and decisions; artifacts carry the unverified banner.
  - Given orchd is down, when I view the tabs, then controls disable with honest empty states.
- **Priority:** should
- **Status:** delivered *(SCN-035)*

### ST-029: Set project rules and policy
- **Story:** As P-01, I want editable markdown rules and a spend/confirmation policy per project, so that I govern how agents behave in it.
- **Traces:** JTBD-06, JTBD-07
- **Acceptance criteria:**
  - Given the Rules tab, when I edit rules or the policy (spend cap, confirmation classes, allowed paths) and save, then they persist; external file change/loss shows Accept/Recreate banners.
  - Given invalid input, when I save, then inline validation ("spend cap must be a number", etc.) blocks it.
- **Priority:** should
- **Status:** delivered *(SCN-036)*

### ST-030: Stay informed when a background service degrades
- **Story:** As P-01/P-03, I want honest, non-dismissable banners when a daemon drops or the DB degrades, so that I always know the true persistence state.
- **Traces:** JTBD-08, JRN-09/#1, JRN-09/#3
- **Acceptance criteria:**
  - Given sessiond drops, when it disconnects, then a red self-healing banner shows and clears on reconnect.
  - Given orchd is down, when it drops, then a global red banner shows and every orchd-mutating control disables while reads stay live.
  - Given the DB is in-memory or was reset, when the app connects, then a blunt red banner states changes will not survive a restart / where the damaged copy was saved.
- **Priority:** must
- **Status:** delivered *(SCN-037, SCN-039, SCN-041)*

### ST-031: Upgrade background services without losing work
- **Story:** As P-01, I want a clear upgrade dialog that states what is saved and always leaves a path back, so that upgrading never dead-ends or loses records.
- **Traces:** JTBD-08, JRN-09/#2
- **Acceptance criteria:**
  - Given an incompatible sessiond/orchd, when the upgrade dialog opens, then it states what happens to live sessions / records; Update restarts, Cancel leaves a persistent amber re-entry banner.
  - Given both daemons are incompatible, when dialogs contend, then the sessiond variant takes precedence; a failed upgrade shows an inline permissions hint.
- **Priority:** must
- **Status:** delivered *(SCN-038, SCN-040)*

### ST-032: Recover from crashes and diagnose failures
- **Story:** As P-01, I want a crash recovery screen and a diagnostics log, so that a render crash isn't a dead app and failures survive their toasts.
- **Traces:** JTBD-08, JRN-09/#4, JRN-09/#5
- **Acceptance criteria:**
  - Given a render crash, when a view throws, then a full-viewport "Something broke" card with "Reload app" shows and the crash is recorded (scrubbed) in Diagnostics.
  - Given the Diagnostics panel, when I open it, then it lists up to 200 newest-first events (secrets/home paths scrubbed) with "Copy support bundle" and "Clear".
- **Priority:** should
- **Status:** delivered *(SCN-042, SCN-043)*

### ST-033: Keep the machine awake during live sessions *(stated 2026-07-22)*
- **Story:** As P-01, I want the app to prevent system sleep while sessions run, so that long unattended agent runs are never killed by macOS power management.
- **Traces:** JTBD-10, JRN-10/#5
- **Acceptance criteria:**
  - Given keep-awake is enabled and ≥1 live session exists, when the system would idle-sleep, then sleep is prevented and an indicator shows the assertion is active.
  - Given zero live sessions (or the toggle off), when sessions end, then the assertion releases and normal power behavior resumes.
  - Given the OS denies the assertion, when it fails, then the failure is surfaced honestly (banner/toast + Diagnostics), never a silent fake "awake".
- **Priority:** must
- **Status:** validated *(SCN-045 validated)*

### ST-034: Delegate decision authority to a CEO agent *(stated 2026-07-22)*
- **Story:** As P-01, I want a supervisor agent that answers terminal agents' questions within the authority I delegated (project rules, policy caps, confirmation classes), so that agents don't stall waiting for me.
- **Traces:** JTBD-10, JRN-10/#1, JRN-10/#2, JRN-10/#3
- **Acceptance criteria:**
  - Given a project with rules/policy and CEO enabled with a delegation scope, when a terminal agent asks a question within a delegated class, then the CEO answers it autonomously and the session continues without operator input.
  - Given a question outside the delegated scope (or over a cap), when it arrives, then the CEO does NOT answer; the session parks as "needs you" with a persistent, visible escalation signal.
  - Given any autonomous decision, when it is made, then it is logged with question, answer, basis (rule/policy line), and timestamp — reviewable later.
- **Priority:** must
- **Status:** validated *(SCN-046, SCN-047, SCN-048 validateds)*

### ST-035: Continue the workflow after a task ends *(stated 2026-07-22)*
- **Story:** As P-01, I want the CEO to hand a finished agent its next task per the project workflow, so that the pipeline keeps moving while I'm away.
- **Traces:** JTBD-10, JRN-10/#4
- **Acceptance criteria:**
  - Given a project workflow and a non-empty backlog, when an agent completes its task, then the CEO selects the next task (respecting priority) and starts the agent on it, recording the hand-off.
  - Given an empty backlog or an ambiguous next step, when the task ends, then the session parks in an honest idle/"needs you" state — never a fake busy or a silent stop.
  - Given a hand-off failure, when it occurs, then it is surfaced (toast + Diagnostics + decision log), and the session stays recoverable.
- **Priority:** must
- **Status:** validated *(SCN-049 validated)*

### ST-036: Review what happened while I was away *(stated 2026-07-22)*
- **Story:** As P-01/P-03, I want a decision log and a "since you left" digest, so that I can audit every autonomous decision and catch up in one glance.
- **Traces:** JTBD-10, JRN-10/#6
- **Acceptance criteria:**
  - Given autonomous decisions occurred, when I open the decision log, then each entry shows question, answer, basis, outcome, and timestamp, newest-first.
  - Given I return after time away, when I open the app, then a digest summarizes decisions made, tasks completed/started, and open escalations.
- **Priority:** should
- **Status:** validated *(SCN-050 validated)*

### ST-037: Prioritize tasks — urgent vs normal *(stated 2026-07-22)*
- **Story:** As P-01, I want a priority field on tasks, so that urgent work is visibly distinct and the workflow consumes it first.
- **Traces:** JTBD-06, JTBD-10, JRN-10/#4
- **Acceptance criteria:**
  - Given a task, when I set priority (urgent/normal), then it persists, and urgent tasks are visually distinct and sort ahead within their status group.
  - Given workflow continuation (ST-035), when the CEO picks the next task, then urgent tasks are consumed before normal ones.
- **Priority:** should
- **Status:** validated *(SCN-051 validated)*

### ST-038: See token and cost usage per agent and project *(stated 2026-07-22)*
- **Story:** As P-01, I want token and cost statistics per agent and per project over selectable ranges, so that I know where the money goes.
- **Traces:** JTBD-11, JRN-11/#2, JRN-11/#3
- **Acceptance criteria:**
  - Given usage occurred, when I open the stats view and pick a range (All | 30d | 7d), then tokens and cost render per project and per agent for that range.
  - Given no data for a range, when I view it, then an honest empty state shows — never zeros styled as data.
- **Priority:** must
- **Status:** validated *(SCN-052 validated)*

### ST-039: See output statistics — commits and code *(stated 2026-07-22)*
- **Story:** As P-01, I want commit counts and code-change stats per project per period, so that I can see what the operation actually produced.
- **Traces:** JTBD-11, JRN-11/#4
- **Acceptance criteria:**
  - Given workspaces with git history, when I open output stats for a range, then commits and code delta render per project.
  - Given a workspace without git (or git read fails), when stats load, then that project shows an honest "no git data" state, not fabricated zeros.
- **Priority:** should
- **Status:** validated *(SCN-053 validated)*

### ST-040: Activity overview dashboard *(stated 2026-07-22)*
- **Story:** As P-01, I want a single activity dashboard (density heatmap + range switcher), so that "what happened lately" is one glance, not an investigation.
- **Traces:** JTBD-11, JRN-11/#2
- **Acceptance criteria:**
  - Given the dashboard, when it opens, then an activity heatmap and range switcher render with loading/empty/error states handled honestly.
  - Given the orphan design-system atoms (SegmentedPill, Heatmap — COV-01), when this ships, then they are wired here (their intended home) and COV-01 closes.
- **Priority:** should
- **Status:** validated *(SCN-052 validated)*

### ST-041: Per-project documentation surface *(stated 2026-07-22)*
- **Story:** As P-01, I want a documentation area inside each project, so that a project's docs live with the project and both I and the agents can consult and maintain them in place.
- **Traces:** JTBD-06, JRN-07/#2
- **Acceptance criteria:**
  - Given a project's Docs tab, when I create or edit a markdown document, then it persists with the project and renders as formatted markdown.
  - Given a doc file changed or lost externally, when I open the tab, then Accept/Recreate banners surface it (the Rules-tab file pattern) — never silent divergence.
  - Given orchd is down, when I view the tab, then mutations disable while reading stays live.
- **Priority:** should
- **Status:** validated *(SCN-054 validated 2026-07-23)*

### ST-042: Triage the autonomous operation from Home *(stated 2026-07-23)*
- **Story:** As P-01/P-03, I want Home to rank everything that needs me — escalations with the agent's actual question visible — and to summarize what happened while I was away, so that I act on the top blocker in seconds without opening each session.
- **Traces:** JTBD-01, JTBD-10, JRN-02/#2, JRN-10/#6
- **Acceptance criteria:**
  - Given escalations exist, when I open Home, then each shows the agent's question text and the escalation reason (out-of-scope class / no next task) without navigating, ranked above running and finished.
  - Given time away with CEO activity, when I open Home, then a "since you left" digest (decisions / hand-offs / open escalations) renders, links to the decision log, and clears once seen.
  - Given running sessions, when I scan Home, then each row shows project · current task · elapsed time.
  - Given a completed hand-off, when I scan Home, then it reads "done {task} → started {next} (CEO)", not just an exited terminal.
  - Given nothing needs me, when I open Home, then an honest "nothing needs you" state shows.
- **Priority:** must
- **Status:** validated *(SCN-055 validated 2026-07-23 — supersedes the current Home triage scenario on ship)*

### ST-043: Per-project auth context for terminals *(stated 2026-07-23)*
- **Story:** As P-01, I want each project's terminals to run under that project's own Claude Code auth context (a specific org/account), so that one project can operate in org A and another in org B without manual `export`s or cross-project credential bleed.
- **Traces:** JTBD-02, JTBD-06, JRN-07/#2
- **Acceptance criteria:**
  - Given a project with a bound auth context, when I spawn a terminal in it, then the child process authenticates under that context (env-injected key/token + per-context config dir), and `claude /status` in two differently-bound projects proves the split.
  - Given a bound secret, when I save it, then it lives in the OS secret store and never appears in project files, git, the command-history strip, or logs; the panel shows only a masked fingerprint + bound org.
  - Given macOS Keychain is process-global, when the panel is shown, then it states honestly that only env-injected (API-key / setup-token) contexts are isolation-guaranteed and writes `forceLoginOrgUUID` as a fail-fast guard against a mismatched interactive `/login`.
  - Given I clear a context, when I spawn a terminal, then it falls back to the ambient shell login (today's behavior).
- **Priority:** should
- **Status:** draft *(SCN-057 draft 2026-07-23; gated behind the A-8/A-9 spike)*

---

## 5. Assumptions & open questions

Marked by risk dimension: **D**esirability / **V**iability / **F**easibility /
**U**sability. Risky-untested bets flagged ⚠.

- **A-1 (U) — CONFIRMED 2026-07-22.** The pull-only Home triage is NOT
  sufficient: the operator explicitly wants prompt visibility of progress and
  questions so agents never sit blocked unseen. Resolved into JTBD-10
  (escalation signal, ST-034/ST-036) — no longer an open assumption.
- **A-2 (D) ⚠** — Restart-survival (JRN-04) is *the* trust-defining moment for
  P-03. If rehydrate ever loses a session, trust is lost disproportionately.
  Worth explicit reliability testing beyond the current PASS.
- **A-3 (U)** — First-run users discover ⌘K capture on their own. The empty
  state does not currently teach the hotkey. Low severity; easy to test.
- **A-4 (D/V) ⚠** — The manual knowledge graph (ST-025, `could`) is worth the
  upkeep effort for a solo builder. No evidence it is used vs. ignored;
  candidate to validate before investing further.
- **A-5 (D)** — Three personas are really one operator in three states. If a
  second real audience emerges (e.g. a teammate, or a non-terminal user), the
  persona layer must split rather than stretch.
- **A-6 (D/U) — RESOLVED 2026-07-22.** The operator confirmed a dedicated
  per-project documentation surface IS wanted (graph + rules + workspace
  files are not enough). Resolved into **ST-041 / SCN-054** under JTBD-06.
- **A-7 (D/F) — RESOLVED 2026-07-22 (operator-stated).** The CEO's decision
  model is defined. **Information sources, in priority order:**
  1. **Configuration** — the delegation scope the operator sets (classes,
     caps — SCN-046);
  2. **Project data** — read access to the project's goals, tasks, ideas,
     insights, graph, and rules;
  3. **Custom CEO rules** — additional operator-defined rules specific to
     the supervisor;
  4. **Operator instruction text** — free-form text the operator writes;
     the CEO must use it as given;
  5. **(future)** MCP tools and other extensions.
  **Decision rule:** for every agent question the CEO decides exactly one of
  two things — *answer itself* (when the answer is grounded in sources 1–4)
  or *delegate to the operator* (escalate). An answer it cannot ground in a
  configured source is an escalation by definition — the CEO never invents
  authority. Remaining build risk: grounding quality — covered by the
  decision log (every answer cites its source) and SCN-047's
  "never guesses" rule.
- **A-8 (F)** — Token/cost data for terminal agents (ST-038) is obtainable:
  Claude Code exposes usage locally (session JSONL / OTel). Feasibility spike
  needed; if a source is unavailable, scope narrows to MCP-call costs
  (already collected via SCN-035 plumbing).
- **A-9 (F/U) — stated 2026-07-23.** Per-project org isolation (ST-043/SCN-057)
  is feasible via env injection at terminal spawn (`ANTHROPIC_API_KEY` /
  `CLAUDE_CODE_OAUTH_TOKEN` + per-context `CLAUDE_CONFIG_DIR`,
  `forceLoginOrgUUID` guard). **Known boundary:** on macOS the Keychain is
  process-global, so an interactive `/login` inside a terminal is NOT isolated
  by `CLAUDE_CONFIG_DIR` — only env-injected key/token contexts are. Spike
  (shared with A-8) must confirm: (1) child-process env plumbing at the spawn
  site, (2) secret-store round-trip without plaintext leakage, (3) that
  `forceLoginOrgUUID` fails fast on mismatch as documented. If (1) is blocked,
  the feature degrades to a documented manual-`export` recipe rather than
  shipping a false isolation guarantee.

## 6. Best-practices applicability

The tagged catalog ([best-practices.md](../../../.claude/plugins/cache/super-ux/super-ux/0.7.0/skills/references/best-practices.md))
is **48 Laws of Subscription App Success** — paywall, freemium, push, and
lifecycle-monetization mechanisms. Builder Pro AI is a **free local macOS
developer tool**: no paywall, no monetization, no push channel, no store
funnel. The vast majority of BP entries are **not applicable**.

Genuinely transferable:
- **BP-003** (lean onboarding, grow by iteration) → JRN-01: the empty state
  already shows a single next action; keep it lean.
- **BP-012** (defaults / "suggest for me" at decision-heavy steps) → project
  creation and research-args seeding already lean this way.
- **BP-040 / BP-041** (concrete activation metric; aha as a sequence) →
  define activation as "reached a live terminal in a real workspace" (JRN-01/#4)
  and instrument setup → aha → habit. Not yet instrumented — opportunity.

Everything paywall/pricing/push/winback-tagged is **N/A** for this product
class and should not be force-fit in future scenario or audit work.

---

## Definition of done

- Layers consistent, IDs stable (P-01..03, JTBD-01..11, JRN-01..11, ST-001..043).
- All inferred entries marked; operator-stated entries carry
  `(stated 2026-07-22)`; risky assumptions flagged in §5.
- Every scenario maps to ≥1 story; every must/should story has ≥1 scenario
  (delivered ones implemented; SCN-045..053 validated by the operator
  2026-07-22; SCN-054 draft awaiting design review).
- **WHY layer validated by the operator 2026-07-22**; open items: A-8
  token-source spike + A-9 org-isolation spike (shared) before ST-038/ST-043
  build; SCN-057 awaiting operator approval. *(A-7 resolved 2026-07-22;
  ST-041/SCN-054 shipped; SCN-055 validated.)*
