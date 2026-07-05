# Builder Pro AI — Product Vision v2 (owner's expansion, 2026-07-06)

Verbatim-in-substance capture of the owner's second vision statement. This EXTENDS (does not
replace) [`2026-07-01-product-vision.md`](2026-07-01-product-vision.md). Docs/specs/roadmap are
audited against BOTH.

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
