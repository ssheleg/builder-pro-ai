# Builder Pro AI — Product Vision v2 (owner's expansion, 2026-07-06)

Verbatim-in-substance capture of the owner's second vision statement. This EXTENDS (does not
replace) [`2026-07-01-product-vision.md`](2026-07-01-product-vision.md). Docs/specs/roadmap are
audited against BOTH.

## The pain this product exists to kill (owner, 2026-07-06)

> «Проблема сейчас — хаос и потеря целей и контекста: всегда 5-6 проектов в работе, у каждого
> разная скорость и разные этапы, всё держать в голове невозможно. Постоянно надо подключаться,
> проверять что делают агенты и нажимать кнопки — хотя всё это можно было бы делать автоматически.»

Translated into product law:
1. **The attention tax is the enemy.** Every "connect and check what the agents are doing" and
   every routine button-press is a defect. The system pulls the owner in (batched escalations,
   morning summaries) — the owner never polls.
2. **Context restoration in seconds.** Opening the panel must answer, for all 5-6 projects at
   once: where is each one, what moved since I last looked, what (if anything) needs ME.
3. **Autonomy is the default, approval is the exception.** Anything that CAN proceed under policy
   does; only policy-gated actions (spec approvals, destructive ops, spend) wait — and they wait
   in ONE inbox, not in N terminal tabs.

North-star metrics: (a) time-to-context on app open (target: <30 s for all projects), (b) owner
interventions per shipped task (target: →1, the approval that matters), (c) hours agents progress
unattended without stalling on a button.

## Mission (owner's words, restated)

**The product's main job: take ANY idea to a working project, and manage/organize the
vibecoding process.** Across all projects the owner must always see: goals, plan tasks, bug-fix
tasks, prioritization, the delivery workflow, monitoring — and the system grows by adding more
tools for custom chains and by connecting MCP servers.

## New first-class concepts

### 1. Ideas
- The owner can add an **idea** either **to an existing project** or **as a new project born
  from the idea**.
- An idea can (optionally, when needed) get **research** via the **prowl.chat MCP server**
  (owner's prowl project: `oxdev`) — competitive/market/SEO intelligence.
- From that research a **task is formed**, which is **decomposed into subtasks** and **designed
  within our development workflow** (the existing brainstorm → spec → plan → subagent pipeline).
- Pipeline: `idea → [research (MCP)] → task → subtasks → design → development`.

### 2. Editable, flexible development workflow (workflow-as-data)
- The development workflow itself must be **updatable by the owner**: add steps to the flow,
  reorder, extend.
- **Recurring actions** are part of workflows. Canonical example (owner's):
  - every 24 h: analyze the project's **production error logs**;
  - errors land in a **study folder** («папка для изучения»);
  - there, **research** runs over the code, the logs, the database, the project's documentation,
    the libraries' documentation — wherever needed;
  - from that research, **tasks enter development** and get **deployed**;
  - net effect: **the project watches its own production errors and fixes them**.
- Same mechanism for **analytics/metrics watching** and **sprint planning**: the system observes
  metrics and plans the sprint.
- The owner wants to **change the flow and keep it flexible for himself** — custom chains,
  composable steps, more tools over time.

### 3. MCP servers as the extension mechanism
- The app connects **MCP servers** as tool providers for agents and workflow steps
  (prowl.chat first; more later).
- Custom chains are built from these tools.

## The home screen (owner, 2026-07-06 — the ideal open-the-app moment)

> «Круто когда я открываю и вижу агентов на каждом этапе в разных проектах — что они делают, могу
> посмотреть прогресс по проекту и накинуть идей в бэклог, добавить настройки для CEO и других
> агентов организации, корректировать вектор разработки и видеть горячие вопросы, которые нужно
> решать человеку.»

Decomposed — the home screen shows, in one view:
1. **Live agent feed** — every agent, on every stage, in every project: who is doing what right now.
2. **Per-project progress** — drill-in from the same view.
3. **Idea quick-capture** — toss ideas into a project backlog (or spawn a project) without leaving.
4. **Agent-org settings** — configure the CEO and the other organization agents (roles, models,
   policies, prompts) — the agent ORG is itself a configurable entity.
5. **Vector steering** — adjust the development direction (goals/priorities) and agents pick it up.
6. **Hot questions** — the human-decision queue (escalations) front and center.

## Cross-project visibility (the control panel promise)
For every project, always visible: goals · tasks from the plan · bug-fix tasks · prioritization ·
delivery workflow state · monitoring. The panel is the single place the owner directs all
projects from.

## Implied capabilities (derived, to be validated in the audit)
- Idea entity + lifecycle (captured → researched → specced → in-dev → shipped).
- Research artifacts as durable, linkable objects (feed the knowledge graph).
- Workflow engine: definitions as data (steps, triggers, schedules), a scheduler (cron-like),
  event triggers, per-step tool bindings (agents, terminals, MCP tools), run history.
- Production feedback loop: log/error ingestion per project, a triage/study queue, research
  tasks, fix tasks, deploy step, verification.
- Metrics/analytics ingestion per project (extends the old S8 idea) feeding sprint planning.
- Delivery/deploy as a workflow step (projects get deployed — the panel must know how).
- MCP client subsystem inside the product (connect/authorize/invoke MCP servers; prowl session
  semantics: stable session_id caching per the server's instructions).
