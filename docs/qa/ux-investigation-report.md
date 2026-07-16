# UX-инвестигейт — единый отчёт (фаза 2)

> Заполняется циклом глубокой проверки по каталогу `docs/qa/ux-first-session-scenarios.md`.
> Один сценарий = одна секция ниже + отметка в чек-листе. Цикл идёт, пока есть ⬜.

**Вердикты:** ✅ OK — работает, обработки/логи на месте · 🟡 UX-GAP — работает, но юзер
страдает (нет фидбека/индикации/пути восстановления) · 🔴 BUG — дефект поведения ·
📄 DOC-GAP — код ок, доки врут/молчат.

## Чек-лист (статус всех сценариев)

| Эпик | Сценарии | Статус |
|---|---|---|
| A — запуск/демоны | A-01 🟡 · A-02 🟡 · A-03 🟡 · A-04 🟡 · A-05 ✅ · A-06 ✅ · A-07 ✅ · A-08 🟡 · A-09 🟡 · A-10 🟡 | 10/10 |
| B — workspace/файлы/терминал | B-01 🟡 · B-02 ✅ · B-03 ✅ · B-04 📄 · B-05 🟡 · B-06 🟡 · B-07 🟡 · B-08 🟡 · B-09 ✅ · B-10 ✅ · B-11 ✅ · B-12 🟡 · B-13 🟡 · B-14 ✅ | 14/14 |
| C — проект | C-01 ✅ · C-02 🟡 · C-03 🟡 · C-04 ✅ · C-05 🟡 · C-06 🟡 · C-07 🔴 · C-08 📄 · C-09 ✅ | 9/9 |
| D — цели/метрики | D-01 ✅ · D-02 🟡 · D-03 ✅ · D-04 ✅ · D-05 🟡 · D-06 ✅ · D-07 🟡 | 7/7 |
| E — идеи (гипотезы) | E-01 ✅ · E-02 ✅ · E-03 ✅ · E-04 ✅ · E-05 🟡 · E-06 ✅ · E-07 🟡 · E-08 🟡 | 8/8 |
| F — research | F-01 ✅ · F-02 ✅ · F-03 🔴 · F-04 🟡 · F-05 🟡 · F-06 🟡 · F-07 ✅ · F-08 🟡 · F-09 🟡 · F-10 🟡 · F-11 🟡 · F-12 ✅ · F-13 ✅ · F-14 ✅ | 14/14 |
| G — инсайты | G-01 ✅ · G-02 🟡 · G-03 ✅ · G-04 ✅ · G-05 🟡 · G-06 🟡 · G-07 ✅ · G-08 🟡 | 8/8 |
| H — задачи (фичи) | H-01 🟡 · H-02 🟡 · H-03 ✅ · H-04 🟡 · H-05 ✅ · H-06 ✅ · H-07 ✅ | 7/7 |
| I — граф | I-01 🟡 · I-02 🟡 · I-03 🟡 · I-04 ✅ · I-05 ✅ · I-06 ✅ · I-07 🟡 · I-08 ✅ · I-09 🟡 | 9/9 |
| J — расширения | J-01 🟡 · J-02 🟡 · J-03 ✅ · J-04 🟡 · J-05 🟡 · J-06 🟡 · J-07 🟡 · J-08 🟡 | 8/8 |
| K — кросс-каттинг | K-01 🟡 · K-02 🟡 · K-03 ✅ · K-04 🟡 · K-05 ✅ · K-06 🟡 · K-07 🟡 | 7/7 |
| **Итого** | | **101/101 — ЦИКЛ ЗАВЕРШЁН** — 43 ✅ · 54 🟡 · 2 🔴 (F-03→BL-89 Critical, C-07→BL-90 Minor) · 2 📄 (C-08 архив UI-недостижим, B-04 удаление root UI-недостижимо) |

## Итог цикла — приоритизация

101/101 проверено. **Ни одного дефекта потери/повреждения данных в happy-path.** Ядро
(проект→цель→идея→research→инсайт→задача) поведенчески корректно; почти все 🟡 — про
**обратную связь, восстановление и индикацию режима**, а не про сломанную логику.

### Tier 0 — Critical (единственный)
- **F-03 / B-01 → BL-89.** `McpConnect` без таймаута ни на одном слое. Зависший MCP-эндпоинт
  клинит ВЕСЬ orchd-конвейер (dispatch последователен, одно общее соединение), баннер
  «недоступен» не появляется, лечится только рестартом orchd. Латентный близнец —
  **B-02/J-05 → BL-91** (OAuth-обмен без таймаута; в v1 недостижим, оживёт с первым провайдером).

### Tier 1 — Important (чинить до внешнего теста)
- **Реконнект-регидрация неполна (K-02 + F-10/F-11/P-24) → BL-92.** `onOrchdUp` рефетчит только
  projects + слайсы открытого проекта; пропускает research-runs, ВСЮ ext-поверхность, audit,
  global-ruleset. Плюс research-run не имеет polling/manual-refresh и boot-reconcile не шлёт пуш —
  прерванный run навсегда «выполняется» на экране (в БД `failed{interrupted}`).
- **Триада молчаливого no-op (P-01/P-02/P-03) → BL-93.** «Новый терминал» / закрыть «×» / «+ Add
  workspace» — fire-and-forget через `void handler()` без catch: отказ проглочен без toast. P-02
  хуже — reject пропускает `manager.dispose` → зомби-таб + утечка xterm (и `removeSession` мёртвый).
- **Молчаливая деградация БД без индикации (A-08/A-09) → BL-94.** Карантин повреждённой orchd.db и
  in-memory-fallback (диск недоступен) — оба без единого UI-сигнала: юзер видит пустой аккаунт или
  работает в неперсистентном режиме, теряя сессию при рестарте. Сигнал только в orchd-логе.
- **Партиал-фейлы без компенсации/идемпотентности (E-07/P-09 + G-08 + P-19) → BL-95.** Spawn-project:
  обрыв после createProject → осиротевший проект+workspace. «В backlog»: сбой между CreateTask и
  SetIdeaLifecycle → задача есть, идея застряла `researching`; ретрай плодит дубли. Корень — нет
  busy-guard от двойного сабмита НИГДЕ (⌘K, CreateProject, ResearchRun, FormInsight, Spawn, +задача).
- **A-10 (orchd): «Отмена» апгрейд-диалога → тупик → BL-96.** `orchdIncompatible` остаётся, но
  `orchdDown=false` → ни один баннер флаг не читает → диалог не вернуть до рестарта (у sessiond
  симметричный возврат есть).

### Tier 2 — capability прошита, но UI-недостижима (решение владельца)
- **C-08 → архив проекта:** verb+бэкенд+тесты есть, кнопки в UI нет (O-3).
- **B-04 → удаление workspace-root:** verb+`LastRoot`-guard+тесты есть, кнопки нет.
- **I-01/P-22 → граф-редактор:** `update_node` готов end-to-end, но узел создаётся с hardcoded
  «Новый узел», формы title/body/rename/edge-label нет — как редактор знаний это стаб (O-7).
- **D-07/O-4 → metric_refs:** сквозной бэкенд, но owner-сеттера нет → fit-context всегда пуст.
- **J-04/O-5 → OAuth:** реестр провайдеров пуст в v1 → «начать OAuth» = гарантированный тупик с
  копией, читающейся как поломка сервиса (нет пометки «скоро», в отличие от stdio/project-scope).

### Tier 3 — Minor UX-полировка (объединяемо)
Ревёрт правок идеи (E-05/P-27) vs эталон GoalTree (D-03); toast-clobber + нет ручного закрытия
(K-01/P-21); сырые англ. Invariant-тексты + UUID в ошибках (C-05/H-05/I-09, вопрос локализации O-2);
error_kind сырым токеном (F-09); provenance инсайт→задача невидим в UI (H-02/F-7); нет empty-state
у ряда списков + loading==empty (P-11/P-12/P-13); consent-recovery недискаверабелен (P-20); нет
per-row сигнала на graph/tool-отказах; import .md переживает rollback (C-07 → BL-90); нет
виртуализации длинных списков + N+1 refreshResearchRuns (K-06); нет лимитов длины ввода (K-04).

### Документация — правки (📄)
- **F-1:** `architecture.md:530` врёт про копию баннера «Навыки» → привести к
  `frontend-conventions.md` (реальная строка = «Навыки — это реестр; они исполняются, когда
  появится агент-оркестр (S6b).»).
- **F-3:** спека S-EXT/CHANGELOG overclaim про stdio-транспорт (UI жёстко http).
- **F-5/BL-61:** locked-копия «у проекта должен остаться workspace» не производится кодом.
- **B-04/C-08:** доки не отмечают, что удаление root / архив проекта UI-недостижимы.
- **B-04 логи / B-05:** 72/77 verbs + все command-handlers без tracing — задокументировать как
  осознанное решение (логируется слоем ниже) или завести на устранение (O-6).

### Открытые вопросы владельцу (из §4 каталога) — блокируют часть Tier-2/3
O-1 маппинг терминов · O-2 политика ru/en · O-3 архив проекта в UI? · O-4 где редактировать metric_refs? ·
O-5 OAuth-секцию скрыть/«скоро»? · O-6 per-verb логи — норма? · O-7 граф-редактор — стаб или дефект? ·
O-8 — на волны цикл разбит, порядок исполнен F→G→E→C→A→H→J→B→I→D→K.

## Реестр вердиктов по подозрениям (итог цикла)

| Подозрение | Вердикт | Действие |
|---|---|---|
| P-01 / P-02 / P-03 | подтверждены Important — триада молчаливого no-op; P-02 хуже (зомби-таб даже на успехе, `removeSession` мёртвый код) | BL-93 |
| P-04 | НЕ баг — промис по контракту не реджектит (B-10 ✅) | — |
| P-05 | Minor — [Повторить] без busy-фидбека | Tier3 |
| P-06 | подтверждён — «Обзор»-таб + submit CreateProject + sidebar-attach + OAuth-код не гейтятся, но падают в честный toast | Tier3 |
| P-07 / P-08 | подтверждены Minor — потеря drag-позиции / фантом ребра при orchdDown; самолечатся на след. refreshGraph | Tier3 |
| P-09 | подтверждён Important — осиротевший проект; но `ProjectsChanged` его показывает (не «молчание») | BL-95 |
| P-10 | подтверждён Minor — failed-fetch артефакта = тот же пустой диалог, что «без ресёрча» | Tier3 |
| P-11 / P-12 / P-13 | подтверждены Minor — нет empty-state / loading==empty / null-навсегда без retry | Tier3 |
| P-14 / P-15 | подтверждены Minor — HomeGoals молча пропускает грузящийся проект; listOps-отказ = пустой селект | Tier3 |
| P-16 / P-17 / P-28 | подтверждены Minor — picker-ошибки через describeOrchdError; сырой message; слитые причины clipboard/export | Tier3 |
| P-18 | НЕ дефект — вердикт без reasoning интенционален (fit_reasoning опционален); архив-причина отдельно | G-07 (📄) |
| P-19 | подтверждён — busy-guard'а нет НИГДЕ; последствия от 2 идей (Minor) до 2 проектов (Important) | BL-95 |
| P-20 | подтверждён Minor — consent-recovery недискаверабелен (ConnectDialog в одном месте) | Tier3 |
| P-21 | подтверждён Important — toast single-slot clobber + нет ручного закрытия | BL-97 (Minor-факт, но частый) / Tier3 |
| P-22 | подтверждён Important — граф-редактор стаб (нет формы/rename/edge-label) | BL / O-7 |
| P-23 | подтверждён Minor — нет `orchd://audit-changed`, аудит только на ремоунте | Tier3 |
| P-24 | подтверждён Important — нет polling/refresh, run застревает визуально | BL-92 |
| P-25 / P-26 / P-27 | подтверждены Minor — bearer без следа; Cancel не откатывает insight; правки идеи без реверта (vs D-03) | Tier3 |
| B-01 | 🔴 Critical — McpConnect без таймаута | BL-89 |
| B-02 | латентный Critical — OAuth-обмен без таймаута, в v1 недостижим | BL-91 |
| B-03 | приемлемо — TrustGrantConsent не в audit_log (сам акт установки гейта) | — |
| B-04 / B-05 | подтверждены — ~72/77 verbs + command-handlers без tracing (логируется слоем ниже) | O-6 / 📄 |
| B-06 | подтверждён Minor — GraphAddEdge post-insert lookup fail → push молча пропущен | Tier3 |
| B-07 | 🔴 Minor латентный — import .md переживает rollback | BL-90 |
| B-08 | подтверждён Minor — XOR-CHECK всплывает как Io, не Validation | Tier3 |
| B-09 | = B-06 | Tier3 |
| B-10 | подтверждён — sessiond `Push::Error` не эмитится в UI (warn-only) | Tier3 |
| F-1 | 📄 architecture.md врёт про копию баннера; frontend-conventions прав | док-правка |
| F-3 / F-5 / F-8 | 📄 подтверждены — stdio overclaim; locked-копия last-workspace не производится; архив UI-недостижим | док-правка |

## Шаблон секции результата

```markdown
### <ID> — <название сценария>

- **Вердикт:** ✅ / 🟡 / 🔴 / 📄 (+severity: Critical / Important / Minor)
- **Проверено:** код-путь (файл:строка → файл:строка), тест/репро (команда + вывод).
- **Обработка ошибок:** есть/нет, честная/проглочена — доказательство.
- **Логи:** что эмитится на success/fail (уровень+поля), секретов нет — доказательство.
- **Что видит пользователь:** фактическое поведение экрана.
- **Дельта от ожидания:** (если есть) что расходится с каталогом.
- **Действие:** ничего / BL-xx заведён / фикс в слайсе <...> / док-правка.
```

---

## Результаты

_(секции добавляются циклом фазы 2)_

## Волна 1 — эпики F, G, E (2026-07-16)

# Эпик F — Research (ресёрч гипотезы): инвестигейт F-01..F-14

> READ-ONLY инвестигейт по каталогу `docs/qa/ux-first-session-scenarios.md` §2 Эпик F.
> Модель: opus. Все пути прослежены UI-контрол → ipc → command → wire → dispatch → модуль.
> Тесты прогонялись через `cargo test -p bpa-orchd --lib <filter>`.

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| F-01 | ✅ OK | — | Добавление сервера: поля-гварды, orchdDown-гейт, success→reset+refresh, error→toast+поля сохранены; stdio/project/oauth disabled «скоро» |
| F-02 | ✅ OK | — | Connect через ConnectDialog: consent→connect→refresh, busy-гвард, error inline+toast диалог открыт; push McpToolsChanged; B-03 приемлемо |
| **F-03** | **🔴 BUG** | **Critical** | **B-01 подтверждён: McpConnect НЕ обёрнут в timeout ни на одном слое → мёртвый/висящий сервер намертво вешает ВЕСЬ orchd-конвейер приложения, без баннера «оркестратор недоступен»** |
| F-04 | 🟡 UX-GAP | Minor | Re-consent при смене URL работает (security-correct), но диалог не объясняет «URL изменился»; ServersTab всегда показывает диалог — юзер не отличает первый consent от повторного |
| F-05 | 🟡 UX-GAP | Minor | P-25: токен сохраняется честно, но единственное подтверждение — 4-сек toast; ряд сервера никогда не показывает «bearer задан» |
| F-06 | 🟡 UX-GAP | Minor | Пустой стейт ResearchRunDialog: 0 подключённых серверов → пустой селект без опций и без CTA к «Расширениям», «Запустить» навсегда disabled |
| F-07 | ✅ OK | — | Невалидный JSON: `JSON.parse` в try/catch ДО вызова → inline-ошибка, `researchStartRun` не зовётся |
| F-08 | 🟡 UX-GAP | Important | Happy-path ок (toast+close+badge через пуши), НО нет in-flight-гварда на «Запустить» (в отличие от ConnectDialog) → P-19 двойной клик = два run + два внешних вызова + двойной spend |
| F-09 | 🟡 UX-GAP | Minor | error_kind ЧЕСТНО показан юзеру (все 11 видов), но как сырой англ. snake_case токен (`policy_cap_exceeded`…) — нет локализации/расшифровки |
| F-10 | 🟡 UX-GAP | Important | Boot-reconcile флипает run в failed{interrupted}, но пуша на буте НЕТ; `orchd://up`-хендлер НЕ рефетчит research runs → бейдж навсегда застревает на «выполняется», хотя в БД failed |
| F-11 | 🟡 UX-GAP | Important | P-24: нет polling нигде, ResearchPane — чистый reader, нет ручного refresh → потерянный пуш = run застрял в pending/running до нового run или рестарта app |
| F-12 | ✅ OK | — | «показать артефакт»: read (не гейтится orchdDown), fetch-fail→toast+экран не меняется; ArtifactViewer + безусловный (D9) баннер «непроверенные данные» |
| F-13 | ✅ OK | — | Preflight честно показывает эффективную политику + «стоимость неизвестна заранее»; over-cap run→failed{policy_cap_exceeded}+audit `policy_deny` |
| F-14 | ✅ OK | — | Полный orchdDown-гейт: «Исследовать»/«Запустить»/insight-форминг disabled; пейн-toggle + «показать артефакт» (чтения) живут |

**Итог по эпику:** 5×✅ OK · 7×🟡 UX-GAP (2 Important + сопутствующие) · 1×🔴 BUG (Critical) · 1×📄 DOC-GAP (F-3, смежно F-01).

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| **B-01** (McpConnect без timeout) | **🔴 Critical — ПОДТВЕРЖДЁН** | F-03 |
| P-10 (ResearchPane fetch-fail → FormInsightDialog с artifact:null) | 🟡 частично подтверждён | F-12 (путь «Сформировать insight», не «показать артефакт»); детально — эпик G |
| P-19 (двойной сабмит) | 🟡 подтверждён для ResearchRunDialog | F-08 |
| P-24 (нет polling — застрявший run) | 🟡 подтверждён | F-11 |
| P-25 (bearer без стойкого подтверждения) | 🟡 подтверждён | F-05 |
| F-3 (stdio picker overclaim) | 📄 DOC-GAP подтверждён | F-01 |
| B-03 (consent не в audit_log) | ✅ приемлемо | F-02 |

---

## F-01 — «Расширения»→Серверы: имя+URL → «+ сервер»

- **Вердикт:** ✅ OK. (Смежно: 📄 DOC-GAP F-3 — см. ниже.)
- **Проверено:** `src/components/ext/ServersTab.tsx:146-170` (`handleAdd`) → `mcpAddServer(name,"http",url,…)` (ipc/orchd) → `OrchdRequest::McpAddServer` → `socket_server.rs` dispatch → `registry::add_mcp_server`.
- **Обработка ошибок:** есть, честная. `addBlocked = name.trim()===""||url.trim()===""` → submit disabled (ServersTab:144, 271). На успехе — `setName("")`/`setUrl("")`/reset scope+auth + `refreshMcpServers()` (162-166). На отказе — `showToast(describeOrchdError(e))` (168), поля НЕ сбрасываются (сброс только в success-ветке) → «поля сохранены» соблюдено.
- **Логи:** на UI-слое нет (toast). На демоне `add_mcp_server` — обычный insert (B-04-класс: пер-верб tracing нет; системное решение, не дефект эпика F).
- **Что видит пользователь:** новый ряд появился, форма сброшена. Транспорт-пикер жёстко `value="http" disabled` (229); `stdio (скоро)`, `проект (скоро)`, `OAuth (скоро)` — disabled-опции. Submit ещё и `disabled={orchdDown}` (271).
- **Дельта от ожидания:** нет по поведению. **F-3:** спека S-EXT/CHANGELOG заявляют ОБА транспорта; бэкенд их и правда поддерживает (`mcp/transport.rs` Stdio, `connect_session`), но UI-пикер выставляет только http, stdio помечен «скоро» — это overclaim в доках/спеке, не дефект кода. Заводится как 📄 DOC-GAP (F-3), вне кода эпика F.
- **Действие:** ничего по F-01; F-3 — док-правка (спека/CHANGELOG должны честно сказать «UI Phase 1 = http-only, stdio в бэкенде есть, в пикере отключён»).

## F-02 — Сервер добавлен: «подключить» → ConnectDialog → «Подключиться»

- **Вердикт:** ✅ OK.
- **Проверено:** `ServersTab.tsx:317` (`setConnectTarget`) → `ConnectDialog.tsx:106-121` (`handleConfirm`): `trustGrantConsent(id,"connect")` → `mcpConnect(id)` → `refreshMcpServers()` → `onClose()`. Бэкенд: `socket_server.rs:1663` (`TrustGrantConsent`) → `persistence.rs:5840 grant_consent` (upsert `consent_grant`); `socket_server.rs:1549 McpConnect` → `mcp::lifecycle::connect` → push `McpToolsChanged{server_id}` (1556).
- **Обработка ошибок:** есть, честная. `busy`-гвард (ConnectDialog:94, 108, 119, `disabled={busy}` 165). Отказ → `describeOrchdError` → inline `role="alert"` (146-150, переживает clobber toast-очереди) + `showToast` + диалог ОСТАЁТСЯ открыт (нет onClose в catch). Порядок consent→connect верен (mcpConnect trust-gated, `Error{Consent}` пока grant не записан).
- **Логи:** `lifecycle.rs:83` `tracing::info!(server_id, tool_count, protocol_version, "mcp: connected")` — без секретов. `trust::authorize` пишет `audit_log{action='connect', decision='allow'}` (доказано тестом `connect_after_consent_caches_tools_and_returns_report`).
- **Что видит пользователь:** тулзы затянуты в кэш, push `orchd://mcp-tools-changed` + явный `refreshMcpServers`, диалог закрыт, ряд показывает `протокол <version>`.
- **B-03:** `grant_consent` (persistence.rs:5846-5859) пишет ТОЛЬКО в `consent_grant`, НЕ в `audit_log`. Но следом идущий `McpConnect`.authorize пишет `action='connect' decision='allow'` — то есть аудитируется фактически выполненное (безопасно-значимое) действие, а не сам грант. Честно и достаточно. Приемлемо (в худшем случае — минорная observability-заметка: отдельная grant-строка дала бы полную историю consent).
- **Действие:** ничего.

## F-03 — Сервер = мёртвый/висящий эндпоинт: «Подключиться» (B-01 — кандидат-баг №1)

- **Вердикт:** 🔴 BUG. **Severity: Critical.** B-01 подтверждён статически на ВСЕХ слоях + усилен архитектурой соединения.
- **Проверено (статически, весь стек):**
  1. **orchd connect-путь без timeout:** `mcp/lifecycle.rs:68-73` — `connect_fn(server, bearer).await` и `session.list_tools().await` БЕЗ `tokio::time::timeout`. Контраст: `mcp/invoke.rs:130` `tokio::time::timeout(timeout, connect_fn(...))` и `:154` `timeout(timeout, session.call_tool(...))` — D12-фикс лёг ТОЛЬКО на invoke-путь.
  2. **rmcp `serve` без timeout:** `crates/mcp/src/client.rs:97,102` — `().serve(transport).await` (initialize-handshake блокирует до ответа сервера, timeout нет).
  3. **reqwest-клиент без timeout:** rmcp 2.2.0 `default_http_client()` (`.../rmcp-2.2.0/src/transport/common/reqwest/streamable_http_client.rs:305-312`) ставит ТОЛЬКО `.pool_max_idle_per_host(0)` + `.redirect(none())` — НЕТ `.timeout()`/`.connect_timeout()`; reqwest по умолчанию без таймаута.
  4. **config без поля timeout:** `StreamableHttpClientTransportConfig` (`.../streamable_http_client.rs:1255-1360`) — поля timeout нет вообще; `from_uri`/`from_config` используют `default_http_client()`.
  ⇒ для HTTP-эндпоинта, который принимает TCP-соединение, но не отвечает на `initialize` (чёрная дыра/файрвол/зависший сервер), `lifecycle::connect` висит бесконечно. **Опровержение «висит вечно» не проходит: таймаута НЕТ ни на одном слое.**
- **Усиление (архитектура):**
  - Серверная диспетчеризация **последовательная на соединение:** `socket_server.rs:264-290` читает один фрейм и `let res = dispatch(&deps,&broadcaster,req).await;` (274) — инлайн-await, БЕЗ per-request `tokio::spawn`. Зависший `McpConnect`-dispatch клинит reader-loop соединения → последующие запросы на этом соединении не читаются НИКОГДА.
  - Фронт использует ОДНО общее orchd-соединение (единый `OrchdClient`/`connection_task`, `src-tauri/src/orchd_client.rs:24-26`). Клиентский `REQUEST_TIMEOUT=30s` (orchd_client.rs:61, 387) ограничивает КАЖДЫЙ запрос, НО на таймаут `request()` лишь возвращает `Disconnected` (389-391) — НЕ рвёт соединение и НЕ реконнектит; `live` остаётся `true`; зависшее-но-открытое сокет-соединение не даёт read-ошибки (`run_connection` рвёт conn только на read/write-error/EOF, orchd_client.rs:715-729) → `orchd://down` НЕ фаерится.
- **Что видит пользователь (в ConnectDialog):** есть busy-стейт (кнопка `disabled={busy}`, opacity 0.6 — но БЕЗ явного «Подключение…»/спиннера). «Отмена» и `Escape` НЕ гейтятся `busy` (ConnectDialog:100, 156) → диалог отменяем. Через ~30с клиентский REQUEST_TIMEOUT фаерит → catch показывает generic «оркестратор недоступен» (вводит в заблуждение: orchd жив, завис MCP-сервер), busy=false, диалог остаётся. **НО:** серверная dispatch-задача продолжает висеть вечно → всё orchd-соединение приложения намертво заклинено: любая последующая orchd-операция (капчур идеи, загрузка табов, смена проекта, другой research) висит 30с и падает как «disconnected», БЕЗ баннера OrchdDownBanner (клиент по-прежнему считает себя подключённым). Восстановление — только рестарт orchd/приложения (либо, для не-чёрнодырных серверов, ~75с OS-TCP-timeout, после чего reader может разгрестись).
- **Обработка ошибок:** отсутствует на connect-пути (нет таймаута → нет честной деградации). Клиентский 30с-фолбэк маскирует симптом (generic Disconnected), но не лечит клин и врёт про причину.
- **Логи:** на успехе `info "mcp: connected"`; на зависании — НИЧЕГО (задача просто висит, не логируется).
- **Тест-доказательство:** `cargo test -p bpa-orchd --lib call_tool_connect_that_never_resolves_times_out_not_hangs` → **ok** (invoke-путь ограничен D12). Греп `crates/orchd/src/mcp/lifecycle.rs` на `timeout`/`never_resolves`/`pending::` → только doc-comment и test-setup `timeout_ms:5000`, НИ ОДНОГО `tokio::time::timeout` и НИ ОДНОГО timeout-теста → connect-путь имеет нулевое покрытие таймаута. Тот же паттерн фикса (`std::future::pending()` + timeout-ассерт) существует ТОЛЬКО в `invoke.rs`.
- **Дельта от ожидания:** каталог ждал «ошибка по таймауту сервера»; реально — бесконечный зависон dispatch + вечный клин всего orchd-конвейера + дишонест-состояние соединения (нет `orchd://down`).
- **Действие:** BL-строка (🔴 Critical). Фикс: обернуть `connect_fn`/`list_tools` в `lifecycle::connect` в `tokio::time::timeout(server.timeout_ms, …)` (перенести D12-паттерн из invoke.rs), с `McpError::Timeout` → `classify_error_kind`="timeout"; добавить `never_resolves`-тест зеркально invoke. Опционально усилить: клиентский REQUEST_TIMEOUT должен помечать соединение неживым/реконнектить, чтобы клин не был «тихим».

## F-04 — Подключённый сервер: сменить URL → «подключить»

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `mcp/mod.rs:413-424 connect_action` (Http → `Action::Connect{fingerprint = url}`), `mod.rs:430-436 fingerprint_for`; `trust::authorize` сверяет grant.fingerprint с текущим URL. После смены URL fingerprint не совпадает → `authorize` Deny → `OrchdMcpError::ConsentRequired` (`lifecycle.rs:59-61`). ServersTab маршрутизирует КАЖДЫЙ connect через ConnectDialog (ServersTab:317, док-коммент ConnectDialog:76-79).
- **Обработка ошибок:** есть, честная (security-correct: старый bearer/grant не утекает на новый URL — доказано `connect_after_url_change_is_denied_and_bearer_is_never_sent`).
- **Что видит пользователь:** снова открывается ConnectDialog. Текст generic — «Приложение подключится к этому MCP-серверу…» (ConnectDialog:142-144), НЕ объясняет «URL изменился, подтвердите заново». Т.к. диалог показывается на любой connect, юзер вообще не отличает первичный consent от повторного.
- **Дельта от ожидания:** каталог спрашивал «Понимает ли юзер, ПОЧЕМУ снова спрашивают» → нет, причина (fingerprint mismatch) не сообщается.
- **Действие:** UI-улучшение (Minor): в ConnectDialog показывать «endpoint изменился с X на Y — подтвердите заново», когда есть предыдущий grant с иным fingerprint. Не блокер.

## F-05 — Сервер bearer: ввести токен → «задать токен»

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-25 подтверждён.)
- **Проверено:** `ServersTab.tsx:200-212 handleSetBearer` → `mcpSetServerBearer(id, token)` → Keychain (`bpa_secrets`). Токен `.trim()`, пустой → no-op (202); на успехе `setBearerDrafts(...,"")` (207) + `showToast("Токен сохранён")`; на отказе `showToast(describeOrchdError)`.
- **Обработка ошибок:** есть, честная. Отказ Keychain → `Io`-toast. Токен НИКОГДА не переотображается (input `type="password"`, драфт очищается — 205-207).
- **Логи:** секрет не логируется (по контракту `bpa_secrets`).
- **Что видит пользователь:** 4-секундный toast «Токен сохранён» — и всё. Ряд сервера показывает transport/scope/protocol, но НЕ показывает, что bearer задан (`auth_kind` не рендерится в ряду; поля «токен есть» нет). После исчезновения toast — нулевой стойкий след. Refresh не нужен (нечего обновлять) — здесь честно.
- **Дельта от ожидания:** каталог/P-25 — «после toast нет никакого следа». Подтверждено.
- **Действие:** UI (Minor): показывать в ряду индикатор «bearer задан» (по `auth_kind==='bearer'` + факту наличия секрета). Не блокер.

## F-06 — Идея в проекте: «Исследовать» → ResearchRunDialog (пустой стейт)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `ResearchRunDialog.tsx:204 connectedServers = mcpServers.filter(s => s.enabled && s.protocolVersion !== null)`. Селект сервера (253-267): дефолт-опция «выбрать сервер…» + опции только по connectedServers.
- **Обработка ошибок:** н/д (пустой стейт — не ошибка).
- **Что видит пользователь:** при 0 подключённых серверов — селект содержит ТОЛЬКО «выбрать сервер…», ни одной опции. Селект инструмента `disabled` (serverId===""). Preflight-блок не рендерится (308). «Запустить» — `submitBlocked` (serverId==="") навсегда disabled. НЕТ CTA/подсказки «подключите сервер в Расширения→Серверы».
- **Дельта от ожидания:** каталог просил «CTA к Расширениям? Пустой стейт диалога». Диалог — тупик без объяснения, что делать.
- **Действие:** UI (Minor): при `connectedServers.length===0` показать empty-state-строку с ссылкой/подсказкой на «Расширения»→«Серверы». Args-поле уже честно засеяно (seedArgs из title/body).

## F-07 — Диалог: невалидный JSON в args → «Запустить»

- **Вердикт:** ✅ OK.
- **Проверено:** `ResearchRunDialog.tsx:212-233 handleSubmit`: `const raw = argsDraft.trim()===""?"{}":argsDraft; try { JSON.parse(raw) } catch { setArgsError("аргументы должны быть валидным JSON"); return; }` — `return` ДО `researchStartRun`.
- **Обработка ошибок:** есть, честная. Вызова НЕТ; inline `role="alert"` (302-306); ошибка сбрасывается при правке (296-298).
- **Что видит пользователь:** красная строка «аргументы должны быть валидным JSON», run не стартует, диалог открыт.
- **Действие:** ничего.

## F-08 — Диалог: «Запустить» (happy) + двойной клик (P-19)

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** Happy-path ✅, но double-submit подтверждён.
- **Проверено:** `handleSubmit` (212-233) → `researchStartRun(idea.id, serverId, toolName, raw)` → `refreshResearchRuns` → toast «Запуск исследования начат» → onClose. Бэкенд `socket_server.rs:1936 ResearchStartRun` → `research::start_run` (mod.rs:542) → `start_research_run` (одна tx: insert pending + флип idea captured→researching, mod.rs:202-258) → `tokio::spawn(run_research)`; пуши `ResearchRunsChanged` из драйвера (mod.rs:448, 498).
- **Обработка ошибок happy/fail:** отказ → `describeOrchdError` → inline `role="alert"` (326-330) + toast, диалог открыт. OK.
- **P-19 (double-submit):** на «Запустить» НЕТ in-flight/busy-гварда. `submitBlocked = orchdDown || serverId==="" || toolName===""` (236) — ни одно не становится true во время await. В отличие от ConnectDialog (`disabled={busy}`) и CreateProjectDialog. Быстрый двойной клик до `onClose()` → два `researchStartRun` → два research_run + два фоновых `call_tool` + потенциальный двойной spend/rate-hit.
- **Что видит пользователь:** happy — бейдж идеи `ожидание→выполняется→готово` через пуши (idea flip captured→researching виден: lifecycle-чип «в исследовании»). При двойном клике — два ряда в ResearchPane, два фоновых вызова.
- **Логи:** драйвер — `info "research: run completed"` / `warn "research: run failed"` (mod.rs:463-494), без секретов/args.
- **Дельта от ожидания:** double-submit не защищён (как и во всех диалогах — P-19), но здесь цена — реальный внешний вызов и spend.
- **Действие:** BL/фикс (Important): добавить `busy`-гвард на «Запустить» (зеркально ConnectDialog).

## F-09 — Run завершился ошибкой: открыть ResearchPane (виден ли error_kind?)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `ResearchPane.tsx:200-204` — для `status==="failed"`: `{run.errorKind ?? "неизвестная ошибка"}`. Источник kind: `research/mod.rs:508-520 classify_run_error` → `policy_cap_exceeded | tool_disabled | consent_required | (Mcp→classify_error_kind: transport|protocol|timeout|tool_error|auth) | secret_error | internal_error`; + boot-reconcile `interrupted`. Итого 11 видов.
- **Обработка ошибок:** есть, ЧЕСТНАЯ — юзер видит САМ error_kind (не проглочен, не заменён на общее «ошибка»). Это плюс.
- **Что видит пользователь:** бейдж «ошибка» + строка с сырым англ. токеном (`timeout`, `policy_cap_exceeded`, `tool_error`, `interrupted`…). НЕТ карты-локализации (в отличие от `RESEARCH_STATUS_LABEL`) → не человекочитаемо.
- **Дельта от ожидания:** каталог спрашивал «человекочитаем ли он» → нет; честно, но сыро.
- **Действие:** UI (Minor): карта `ERROR_KIND_LABEL` (ru + краткое объяснение/следующий шаг). Не блокер.

## F-10 — Run в running: перезапустить orchd → failed{interrupted}; пуш при буте?

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.**
- **Проверено:**
  - Boot-reconcile: `boot.rs:161-170 reconcile_interrupted` → `research/mod.rs:368-375 reconcile_interrupted_research_runs` — один `UPDATE ... WHERE status IN ('pending','running')` → failed/interrupted. Вызывается `boot.rs:194` ДО `Arc::new(Mutex::new(db))` и ДО `serve()`. Логирует count (`warn`/`info`), **НО broadcaster на этот момент ещё не существует → пуша `ResearchRunsChanged` на буте НЕТ.**
  - Реконнект-регидрация: `App.tsx:248-276 onOrchdUp` рефетчит `refreshProjects` + (если проект открыт) `refreshGoals/Tasks/Ideas/Insights/Ruleset/Graph` — **`refreshResearchRuns` НЕ вызывается ни для одной идеи.**
  - Заполнение: `IdeasList.tsx:391-396` рефетчит `refreshResearchRuns(idea.id)` ТОЛЬКО если `!(idea.id in researchRunsByIdea)` — при перезапуске ключ уже присутствует (устаревший) → повторного фетча нет даже при ремоунте ряда.
- **Что видит пользователь:** бейдж идеи и ResearchPane продолжают показывать «выполняется», хотя в БД run = failed{interrupted}. **Экран активно врёт о состоянии.** Путь восстановления без рестарта приложения отсутствует (пуш на буте нет; up-хендлер research не рефетчит; eager-фетч пропускает присутствующий ключ; polling нет).
- **Обработка ошибок:** реконсиляция в БД честная; UI-слой не узнаёт результат.
- **Логи:** `boot-reconcile: flipped interrupted research runs to failed` (warn, count) — только в демоне.
- **Дельта от ожидания:** каталог «после бута run = failed{interrupted}; пуш при буте не шлётся — экран обновится только по refresh». Реально — refresh, который это исправил бы, на реконнекте НЕ вызывается.
- **Действие:** BL/фикс (Important): в `onOrchdUp`-хендлере (App.tsx) очистить/рефетчить `researchRunsByIdea` (зеркально goals/tasks/ideas/graph, которые ТАМ рефетчатся) — минимально сбросить слайс, чтобы IdeasList eager-fetch перезаполнил. (Опционально: эмитить `ResearchRunsChanged` после boot-reconcile, но broadcaster на буте недоступен — проще лечить на клиенте.)

## F-11 — Run running, push потерян: ждать (P-24)

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (P-24 подтверждён.)
- **Проверено:** `ResearchPane.tsx` — чистый reader слайса `researchRunsByIdea[idea.id]` (док-коммент 73-94: «never fetches the run LIST itself»). Ни `setInterval`, ни polling, ни кнопки ручного refresh нигде в research-UI. Обновление статуса зависит исключительно от пуша `orchd://research-runs-changed` → `App.tsx:239-243` → `refreshResearchRuns(ideaId)`.
- **Что видит пользователь:** при потерянном пуше run застревает на `ожидание`/`выполняется` навсегда — до старта НОВОГО run по этой идее (что зовёт `refreshResearchRuns`) или рестарта приложения. Нет manual-refresh, нет polling, нет индикации «данные могли устареть».
- **Обработка ошибок:** н/д (потеря пуша не даёт ошибки).
- **Дельта от ожидания:** каталог/P-24 — «застрял навсегда, нужен manual refresh». Подтверждено; manual refresh отсутствует.
- **Действие:** BL/фикс (Important): либо лёгкий polling `researchGetRun`/`researchListRuns` пока есть pending/running run в открытом пейне, либо кнопка «обновить» в ResearchPane. Тот же корень, что F-10.

## F-12 — Run done: «показать артефакт»

- **Вердикт:** ✅ OK.
- **Проверено:** `ResearchPane.tsx:129-132 handleShowArtifact` → `ensureArtifact` (115-127) → `mcpGetArtifact(run.artifactId)` → `socket_server.rs:1649 McpGetArtifact` → `get_artifact`. Рендер `ArtifactViewer` (`src/components/ext/ArtifactsTab.tsx:92`, defaultOpen) — REUSE, не реимплементация.
- **Обработка ошибок:** есть, честная. Fetch-fail → `showToast(describeOrchdError)` + `return null` → `shown` не ставится → экран НЕ меняется (соответствует P-10 honest-degradation для read-пути). «показать артефакт» НЕ гейтится `orchdDown` (плоский read) — верно (F-12/F-14).
- **Что видит пользователь:** ArtifactViewer с безусловным (D9) баннером «⚠ непроверенные данные» (`ArtifactsTab.tsx:119-121`, `artifact.isUntrusted` всегда true по конструкции — `insert_artifact` всегда `is_untrusted=1`, доказано invoke-тестом `artifacts[0].is_untrusted`).
- **P-10 (смежно):** для read-пути «показать артефакт» fetch-fail → экран не меняется (ОК). НО путь «Сформировать insight» (`handleFormInsightFromDone`, 134-137) на fetch-fail всё равно открывает FormInsightDialog с `artifact:null` — неотличимо от «без ресёрча». Это ядро P-10; детальный вердикт — эпик G (G-01/G-02), здесь отмечено.
- **Действие:** ничего по F-12 (P-10 — в эпик G).

## F-13 — Политика cap задана: запустить run дороже капа

- **Вердикт:** ✅ OK.
- **Проверено:** Preflight `ResearchRunDialog.tsx:308-324` — `effectivePolicy` (138-146, most-specific-wins server>project>global, зеркалит `trust::resolve_policy`): показывает «область лимита», «лимит расходов $X / не задан», «лимит вызовов/мин», + честную ноту «стоимость внешнего вызова обычно неизвестна заранее — оркестратор остановит вызов, только если он превысит текущий лимит» (319-322). Over-cap run: `run_research` → `call_tool` → `trust::check_policy_caps` Deny → `OrchdMcpError::PolicyCapExceeded` → `classify_run_error`="policy_cap_exceeded" → `set_research_run_failed`. Audit `policy_deny` пишется (доказано `call_tool_on_a_rate_capped_server_is_denied_as_policy_cap_exceeded_with_no_dispatch`, ассерт `audit_log WHERE action='policy_deny'`).
- **Обработка ошибок:** есть, честная — preflight не врёт (не выдаёт «unlimited» за «unset»: `null` → «не задана»), вызов не дороже капа не блокируется преждевременно.
- **Что видит пользователь:** до запуска — честный лимит; при провале — бейдж «ошибка» + `policy_cap_exceeded` (сырой токен — см. F-09-оговорку). Связь «здесь лимит» ↔ «вот почему failed» есть, но юзер должен сам сопоставить (preflight-скоуп ↔ error_kind).
- **Логи:** `warn "research: run failed" error_kind="policy_cap_exceeded"`; audit-строка `policy_deny`.
- **Действие:** ничего (косметика — F-09-локализация улучшила бы связь).

## F-14 — orchd down: весь research-флоу

- **Вердикт:** ✅ OK.
- **Проверено (полный обход гейтов):**
  - «Исследовать» (триггер): `IdeasList.tsx:277 disabled={disabled}` (disabled=orchdDown, 499).
  - ResearchRunDialog «Запустить»: `submitBlocked = orchdDown || ...` (236, 344) — гейт независим от триггера (док-коммент 163-166).
  - ResearchPane insight-форминг: «Сформировать insight» / «без ресёрча» `disabled={disabled}` (178, 191), disabled=orchdDown прокинут `IdeaRow`→`ResearchPane` (IdeasList:335, 499).
  - Чтения/вью живут: пейн-toggle (`idea-research-toggle`, IdeasList:288-295 — чистый view-toggle, без гейта), «показать артефакт» (read, не гейтится — ResearchPane:167-174).
- **Обработка ошибок:** мутации при down недостижимы; чтения при down падают в тот же честный toast.
- **Что видит пользователь:** все мутирующие research-контролы disabled; просмотр run/артефактов и переключение пейна работают.
- **Действие:** ничего.

---

## Сводка ключевого

1. **F-03 / B-01 — 🔴 Critical, главная находка.** McpConnect-путь не обёрнут в timeout НИ НА ОДНОМ слое (lifecycle.rs / rmcp `serve` / reqwest default client / config). D12-фикс лёг только на `invoke::call_tool`. Из-за последовательной серверной диспетчеризации (socket_server.rs:274) + единственного общего клиентского orchd-соединения зависший MCP-сервер намертво клинит ВЕСЬ orchd-конвейер приложения, при этом `orchd://down`-баннер не появляется (клиент считает себя подключённым). Диалог отменяем (Cancel/Escape) и через 30с показывает вводящую в заблуждение «disconnected»-ошибку, но клин соединения остаётся до рестарта orchd.
2. **F-10 / F-11 — 🟡 Important.** Research-run, прерванный рестартом orchd, навсегда показывает «выполняется» (хотя в БД failed{interrupted}): пуша на boot-reconcile нет, а `onOrchdUp`-хендлер (App.tsx) рефетчит все слайсы, КРОМЕ research runs. Плюс полное отсутствие polling/manual-refresh (P-24) → любой потерянный пуш = визуально застрявший run.
3. **F-08 — 🟡 Important.** ResearchRunDialog «Запустить» без in-flight-гварда (в отличие от ConnectDialog) → двойной клик = два внешних вызова + двойной spend.
4. **Мелочи-UX (🟡 Minor):** F-04 (re-consent без объяснения «URL изменился»), F-05/P-25 (bearer без стойкого подтверждения), F-06 (пустой ResearchRunDialog без CTA), F-09 (error_kind честный, но сырой англ. токен).
5. **Хорошо (✅):** F-01/F-02/F-07/F-12/F-13/F-14 — обработка честная, orchdDown-гейт держит, untrusted-баннер безусловный, preflight-политика честная, audit пишется. F-3 — 📄 DOC-GAP (спека заявляет оба транспорта, UI http-only).

**Не удалось проверить рантаймом:** реальный зависон живого HTTP-эндпоинта (нет стенда с «принимает-но-не-отвечает» сервером) — вердикт F-03 построен статически на исходниках всех 4 слоёв + контрастном тесте invoke-пути (`call_tool_connect_that_never_resolves_times_out_not_hangs` — ok) и грепе lifecycle.rs (ноль timeout-кода/тестов). Точный тайминг клиентского клина (30с × N vs OS-TCP ~75с) зависит от природы «мёртвости» сервера.

# Эпик G — Инсайты (G-01…G-08). Результаты инвестигейта

Репо: `/Users/sshlg/DATA/builder-pro-ai` (main, v0.7.0). Read-only.
Тесты прогнаны: `npx vitest run src/components/idea/FormInsightDialog.test.tsx
src/components/InsightsList.test.tsx src/components/idea/ResearchPane.test.tsx` → **30 passed**;
`cargo test -p bpa-orchd --lib insight` → **22 passed** (включая
`research::graph_ingest_tests::*`).

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть |
|---|---|---|---|
| G-01 | ✅ OK | — | fit-context честен; отказ загрузки целей/графа → toast (refreshGoals/refreshGraph); пустой граф/цели показаны честными пустыми состояниями. Минорно: панель информационная, empty==fail визуально; `orchdGraphNeighborhood` без `.catch` (в первой сессии практически недостижим). |
| G-02 | 🟡 UX-GAP | Minor | Сам failed-путь («без ресёрча») честен: `artifact:null`, пустой prefill. НО **P-10**: сбой fetch артефакта на `done`-пути открывает ТОТ ЖЕ пустой диалог `artifact:null` — неотличимо; различает только транзиентный toast, выстреливший ДО открытия диалога. |
| G-03 | ✅ OK | — (📄 F-4 Minor) | 3 стадии корректны, каждая с inline+toast на отказе, диалог не закрывается. «Принять» без toast, но появление кнопки «В backlog» + строка статуса — видимый фидбек. F-4: CHANGELOG терсит «accepting … and forming a task» — не заявляет «один клик», но и не раскрывает два раздельных owner-клика. |
| G-04 | ✅ OK | — | accept сеет ровно один `entity_ref`-узел (тест `accepting_a_research_insight_seeds_exactly_one_entity_ref_node`); re-accept после архива → Conflict benign (тест); orphan → no-op ingest (тест). Переход в таб «Граф» ремоунтит `GraphCanvas` → безусловный `refreshGraph` → узел виден. Латентно (Minor): accept шлёт только `InsightsChanged`, НЕ `GraphChanged`; ingest-fail (не-Conflict) — warn-only, но практически недостижим. |
| G-05 | 🟡 UX-GAP | Minor | **P-26**: после «Создать» «Отмена» закрывает диалог, но insight уже создан (закоммичен + `refreshInsights`) — отката нет. Есть косвенные сигналы (toast «Инсайт создан», локнутые поля, строка статуса, кнопка «Принять»), но лейбл «Отмена» двусмыслен. |
| G-06 | 🟡 UX-GAP | Minor | «В backlog» для идеи-сироты `disabled` (`backlogBlocked` включает `idea.projectId===null`), без прямой подсказки ПОЧЕМУ рядом с кнопкой; есть лишь косвенная нота в fit-context-панели «идея не привязана к проекту — контекст недоступен». |
| G-07 | ✅ OK | — (📄 Minor) | UI: архив без причины блокирован client-side («нужна причина архивации»); вердикт без reasoning проходит — **интенционально** (fit_reasoning опционален и на сервере). P-18 — не дефект (разные поля/требования). DOC-GAP: комментарии в `InsightsList.tsx` заявляют серверную enforcement архив-причины «(spec §5.2)», которой НЕТ. |
| G-08 | 🟡 UX-GAP | Important | `handleBacklog`: `orchdCreateTask` → `orchdSetIdeaLifecycle` последовательно, без идемпотентности/компенсации. Сбой между вызовами (падение/дисконнект orchd) = задача создана, идея застряла `researching`; повторный клик «В backlog» ПЕРЕзапускает `orchdCreateTask` → **дублирующая задача**. Нет guard от двойного сабмита (**P-19**) — быстрый двойной клик тоже дублирует. |

---

## Результаты

### G-01 — Run done → «Сформировать insight» → fit-context (цели+metric_refs+граф-соседство)

- **Вердикт:** ✅ OK
- **Проверено:** код-путь `ResearchPane.handleFormInsightFromDone`
  (`src/components/idea/ResearchPane.tsx:134-137`) → `FormInsightDialog`
  (`src/components/idea/FormInsightDialog.tsx:190-256`). Fit-context: `useEffect`
  (`FormInsightDialog.tsx:228-233`) вызывает `refreshGoals(projectId)`+`refreshGraph(projectId)`;
  соседство — `orchdGraphNeighborhood(ideaNode.id, 1)` (`:249`). Тест
  `FormInsightDialog.test.tsx` кейсы «fit-context: fetches and renders the project's goals with
  metric_refs» и «finds the idea's graph node and fetches its GraphNeighborhood» — зелёные.
- **Обработка ошибок:** есть/частичная. `refreshGoals`/`refreshGraph`
  (`src/store/store.ts:592-635`) обёрнуты в try/catch → `showToast(describeOrchdError(e))` —
  честно. НО `orchdGraphNeighborhood(...).then(...)` (`FormInsightDialog.tsx:249-251`) — **без
  `.catch`**: отказ = unhandled rejection, `neighborhood` остаётся `null` → панель показывает «нет
  связанных узлов». Практически недостижимо в первой сессии: ветка `ideaNode !== null` требует, чтобы
  у самой ИДЕИ уже был граф-узел, а граф-ingest происходит только на insight-ACCEPT (D9), не для
  идей — так что для свежей идеи `ideaNode === null` и fetch не запускается вовсе.
- **Логи:** FE-путь; серверных tracing-логов на `ListGoals`/`GraphListProject`/`GraphNeighborhood`
  нет (системное B-04). Секретов не логируется.
- **Что видит пользователь:** заголовок из идеи, тело из артефакта; правая панель «Контекст для
  оценки» — цели проекта с `metricRefs` и соседи графа. Пустой граф/цели → честные «целей пока нет»
  / «нет узла графа для этой идеи ещё». Панель информационная, ничего не автоприменяет.
- **Дельта от ожидания:** минорная — при ОТКАЗЕ загрузки контекста панель визуально неотличима от
  честно-пустого состояния (различает только toast). Fit-context не блокирует флоу.
- **Действие:** ничего (behavior-OK). Можно завести Minor-BL на `.catch` для
  `orchdGraphNeighborhood` для симметрии с остальными чтениями.

### G-02 — Run failed → «сформировать insight без ресёрча» → artifact:null, честно пустой prefill

- **Вердикт:** 🟡 UX-GAP (Minor) — по подозрению **P-10**.
- **Проверено:** `ResearchPane.handleFormInsightWithoutResearch` (`ResearchPane.tsx:139-141`) →
  `setOpenInsight({runId, artifact:null})`; body-prefill `artifact !== null ? … : ""`
  (`FormInsightDialog.tsx:209`). Тест «a failed run … opens FormInsightDialog with a null artifact
  (Q8)» — зелёный. Путь-двойник: `handleFormInsightFromDone` (`ResearchPane.tsx:134-137`) вызывает
  `ensureArtifact(run)`; при отказе fetch `ensureArtifact` (`:115-127`) ловит, `showToast(...)`,
  **возвращает `null`**, и `setOpenInsight({runId, artifact})` открывает диалог с `artifact:null`.
- **Обработка ошибок:** сам failed-путь корректен и честен. P-10-путь: отказ fetch ПРОГЛАТЫВАЕТСЯ в
  toast, после чего диалог всё равно открывается пустым. Показательна асимметрия в том же файле:
  `handleShowArtifact` (`:129-132`) при `artifact === null` НЕ показывает вьюер (bail), а
  `handleFormInsightFromDone` — открывает диалог безусловно. Тест «an mcpGetArtifact failure
  surfaces via showToast and never renders the viewer» покрывает только show-artifact; P-10-ветка
  form-insight-from-done с упавшим fetch **не покрыта тестом**.
- **Логи:** только FE-toast (`describeOrchdError`), без секретов.
- **Что видит пользователь:** на done-пути с упавшим fetch — мелькнувший toast с ошибкой, затем
  диалог с ПУСТЫМ телом, идентичный намеренному «без ресёрча». Пользователь не отличит «артефакт не
  загрузился» от «артефакта не было».
- **Дельта от ожидания:** каталог спрашивает «Отличим ли от P-10-пути» — ответ **нет**. Единственный
  различитель — транзиентный toast (P-21: один слот, автозакрытие 4с), выстреливший до открытия
  диалога.
- **Действие:** BL-кандидат (Minor): на done-пути при `ensureArtifact === null` либо не открывать
  диалог (как show-artifact), либо показать в диалоге inline-ноту «не удалось загрузить артефакт —
  тело пустое».

### G-03 — Диалог → «Создать» → «Принять» → «В backlog» (3 стадии)

- **Вердикт:** ✅ OK (поведение); 📄 DOC-GAP Minor (**F-4**).
- **Проверено:** `handleCreate` (`FormInsightDialog.tsx:258-273`): `orchdCreateInsight` →
  `orchdSetInsightFitVerdict` → `refreshInsights` → toast «Инсайт создан»; поля лочатся
  `disabled={insight !== null}` (`:342/354/367/386`). `handleAccept` (`:275-287`):
  `orchdSetInsightStatus(accepted, null)` → `refreshInsights` (**без success-toast**). `handleBacklog`
  (`:289-313`): `orchdCreateTask{source:"insight"}` → `orchdSetIdeaLifecycle(specced)` → refresh →
  toast «Задача добавлена в backlog» → `onClose`. Тесты «fires orchdCreateInsight then
  orchdSetInsightFitVerdict, in order», «once created «Принять» fires …», «once accepted «В backlog»
  fires orchdCreateTask then orchdSetIdeaLifecycle(specced), then closes» — зелёные, включая
  assert порядка `invocationCallOrder`.
- **Обработка ошибок:** каждая стадия в try/catch → `setErrorMessage`+`showToast`, диалог остаётся
  открыт (`role="alert"` строка `:442-446`). Тест «a failed create shows the mapped error inline and
  keeps the dialog open» — зелёный.
- **Логи:** серверные пуши `InsightsChanged` (после create/verdict/accept, `socket_server.rs:555`) и
  `TasksChanged{project_id}` (после createTask, `:571`). Пер-верб tracing нет (B-04). Заголовок
  инсайта в graph-ingest-warn НЕ логируется (`persistence.rs:2087-2093`, PII-дисциплина).
- **Что видит пользователь:** «Создать» → toast + поля залочены + строка «статус инсайта: new» +
  появляется кнопка «Принять». «Принять» → строка «статус: accepted» + появляется «В backlog» (toast
  НЕТ, но визуальный переход очевиден). «В backlog» → toast + диалог закрыт.
- **Дельта от ожидания:** нет по поведению. F-4: CHANGELOG (`CHANGELOG.md:51-55`) сжимает флоу в
  «accepting graph-ingests … and forming a task flips the idea researching→specced» — формально НЕ
  утверждает «один клик», но и не раскрывает, что «Принять» и «В backlog» — два отдельных
  owner-клика с раздельными toast (спека S-IDEA §7 — раздельно).
- **Действие:** ничего по коду; F-4 — Minor DOC-правка CHANGELOG при желании.

### G-04 — Инсайт accepted → Открыть «Граф» → узел entity_ref(insight) появился

- **Вердикт:** ✅ OK
- **Проверено:** graph-ingest на accept — `persistence.rs:2076-2096` (`set_insight_status`, после
  `tx.commit()`): при `Accepted` и `Some(project_id)` вызывает `add_entity_ref_node`
  (`graph.rs:403-436`); `Ok | Err(Conflict) => {}`, прочие ошибки — `warn!` и swallow. Тесты
  `research::graph_ingest_tests`: `accepting_a_research_insight_seeds_exactly_one_entity_ref_node`,
  `re_accepting_after_archive_keeps_exactly_one_node_conflict_is_benign`,
  `archiving_a_new_insight_does_not_seed_a_graph_node`,
  `accepting_a_project_less_insight_is_a_no_op_ingest_not_an_error` — все зелёные. Отображение:
  `ProjectPanel.tsx:428` рендерит `<GraphCanvas>` условно (`activeTab === "graph"`) — переход в таб
  ремоунтит компонент, а его mount-effect (`GraphCanvas.tsx:316-319`) безусловно вызывает
  `refreshGraph(projectId)`.
- **Обработка ошибок:** ingest best-effort (статус уже закоммичен). Conflict (re-accept после
  архива — архив не удаляет узел, S4 orphan-on-delete) — benign no-op. Orphan-инсайт — silently
  skipped. Прочий сбой ingest — warn-only, узел отсутствует до re-accept. На практике недостижим:
  `ensure_optional_project_active` уже прошёл до коммита, так что до post-commit ingest проект
  гарантированно активен; остаётся лишь Io/крайне-редкое.
- **Логи:** `warn!(insight_id, entity_type, error, …)` без заголовка (PII) — честно и без секретов.
- **Что видит пользователь:** после accept и перехода в «Граф» — узел инсайта присутствует (свежий
  fetch на mount). Ingest-fail (warn-only) для пользователя необнаружим — но практически
  недостижим.
- **Дельта от ожидания:** совпадает со сценарием. Латентная (Minor) несогласованность: accept
  вещает только `InsightsChanged` (`respond_insight`, `socket_server.rs:549-560`), НЕ `GraphChanged`
  — в отличие от всех прочих граф-мутаций (`socket_server.rs:682-693`). В single-panel first-session
  флоу ремоунт таба это полностью маскирует (узел появляется). Гэп бил бы только по УЖЕ открытой
  граф-поверхности (напр. второе окно), которой в v1 нет. `handleAccept`/`handleBacklog` в диалоге
  тоже не зовут `refreshGraph`.
- **Действие:** ничего для сценария. Латентный BL-кандидат (Minor): вещать `GraphChanged{project_id}`
  из accept-ingest для консистентности с остальными граф-мутациями.

### G-05 — После «Создать» → «Отмена» → диалог закрыт, insight ОСТАЛСЯ

- **Вердикт:** 🟡 UX-GAP (Minor) — по подозрению **P-26**.
- **Проверено:** «Отмена» (`FormInsightDialog.tsx:449-456`) → `onClick={onClose}` — просто закрывает
  оверлей. Никакого `orchdDeleteInsight`/отката. Insight к этому моменту уже создан в БД
  (`handleCreate` → `orchdCreateInsight` закоммичен) и подтянут в стор (`refreshInsights`, `:266`).
  Тест «cancel closes without creating anything» проверяет ТОЛЬКО путь до «Создать» — про отмену
  ПОСЛЕ создания теста нет.
- **Обработка ошибок:** n/a (осознанное поведение — insight по спеке остаётся в «Инсайтах»).
- **Логи:** n/a.
- **Что видит пользователь:** insight остаётся в табе «Инсайты» (виден в `InsightsList`). Косвенные
  сигналы, что создание СОСТОЯЛОСЬ: toast «Инсайт создан», залоченные поля, строка «статус инсайта:
  new», появившаяся кнопка «Принять». Но лейбл «Отмена» двусмыслен — часть пользователей ждёт, что он
  отменит создание.
- **Дельта от ожидания:** сценарий именно это и предполагает (insight остаётся) — так что поведение
  соответствует. Гэп — в понятности: нет явного «инсайт сохранён; закрыть» вместо «Отмена» после
  создания.
- **Действие:** BL-кандидат (Minor): после создания менять лейбл «Отмена» → «Закрыть» или добавить
  строку «инсайт сохранён — доступен в «Инсайтах»».

### G-06 — Идея-сирота → insight → «В backlog» disabled

- **Вердикт:** 🟡 UX-GAP (Minor)
- **Проверено:** `backlogBlocked = orchdDown || insight===null || insight.status!=="accepted" ||
  idea.projectId===null` (`FormInsightDialog.tsx:317-318`); кнопка `disabled={backlogBlocked}`,
  `opacity 0.5` (`:481-483`). Дополнительный guard в `handleBacklog` (`:290`: `if (… ||
  idea.projectId === null) return`). Тест «В backlog» is disabled for an orphan idea» — зелёный
  (кнопка disabled, `orchdCreateTask` не вызывается).
- **Обработка ошибок:** корректно — `orchdCreateTask` требует конкретный `project_id`
  (`persistence.rs:2138-2159`, `project_id TEXT NOT NULL`), у сироты его нет, задачу некуда завести.
- **Логи:** n/a.
- **Что видит пользователь:** кнопка «В backlog» видна, но серая (disabled). Прямой подсказки
  «почему» рядом с кнопкой НЕТ. Единственный косвенный намёк — нота в fit-context-панели «идея не
  привязана к проекту — контекст недоступен» (`:400-403`), но она про контекст, не про кнопку.
- **Дельта от ожидания:** каталог спрашивает «Понимает ли юзер, ПОЧЕМУ disabled (подсказка есть?)» —
  прямой подсказки нет; связь «сирота → нельзя в backlog» пользователю надо додумать.
- **Действие:** BL-кандидат (Minor): `title`/inline-нота у disabled-кнопки «идея без проекта —
  сначала привяжите к проекту».

### G-07 — Таб «Инсайты»: смена статуса; архив с причиной; вердикт+обоснование

- **Вердикт:** ✅ OK (UI); 📄 DOC-GAP (Minor) — по подозрению **P-18**.
- **Проверено:** архив требует причину client-side: `handleStatusChange` (`InsightsList.tsx:174-185`)
  при `archived` НЕ вызывает `onStatusApply`, а показывает inline-поле + «подтвердить архивацию»;
  `handleArchiveConfirm` (`:187-195`) блокирует при пустом reasoning → `setArchiveError(true)` («нужна
  причина архивации»). Вердикт: `onVerdictApply(id, verdict==="" ? null : verdict, verdictReasoning)`
  (`:282-284`) — без валидации reasoning. Тесты `InsightsList.test.tsx` (8) — зелёные.
- **Ключевая проверка серверной enforcement:** `set_insight_status` (`persistence.rs:2033-2099`) —
  **НЕТ** guard «archived требует non-empty resolution_reasoning»: просто
  `resolution_reasoning = COALESCE(?3, resolution_reasoning)`, принимает `None`/`""`. В таблице
  инвариантов спеки S3 §5.2 (`…s3…design.md:370-382`) этого инварианта НЕТ — «archive requires
  non-empty reasoning» упомянут только в списке ФРОНТЕНД-компонентов (`:537`, `InsightsList.tsx`).
  То есть требование — client-only по спеке. Но комментарии в коде (`InsightsList.tsx:13-16` и
  `:146-148`) заявляют «the server enforces this too (spec §5.2)» / «server-side (spec §5.2)» —
  **это неверно**: сервер не enforces, и §5.2 этого не содержит.
- **Обработка ошибок:** UI-путь честен; `handleVerdictApply`/`handleStatusApply` в try/catch →
  `showToast(describeOrchdError(e))` (`:326-348`).
- **Логи:** серверный `InsightsChanged` push; пер-верб tracing нет (B-04).
- **Что видит пользователь:** архив без причины блокируется inline-текстом «нужна причина архивации»;
  вердикт применяется без обоснования.
- **Дельта от ожидания:** P-18 «вердикт без reasoning проходит — несогласованность» — это
  **интенционально, не дефект**: `fit_reasoning` опционален и на сервере (create/verdict его не
  требуют), тогда как `resolution_reasoning` для архива обязателен только в UI. Разные поля, разная
  семантика (вердикт — суждение, архив — журналируемое терминальное действие). Реальная находка —
  DOC-GAP: комментарий врёт про серверную enforcement + defense-in-depth-дыра (прямой
  `SetInsightStatus(archived, "")` сервер примет).
- **Действие:** DOC-правка комментария `InsightsList.tsx` (убрать «server enforces … §5.2»);
  опционально BL: добавить серверный guard архив-причины для defense-in-depth (важно для будущего
  агентного auto-archive, где нет клиента-стража).

### G-08 — После «В backlog» → «Идеи» → идея specced (порядок CreateTask/SetIdeaLifecycle)

- **Вердикт:** 🟡 UX-GAP (Important)
- **Проверено (построчно `handleBacklog`, `FormInsightDialog.tsx:289-313`):**
  1. guard `if (insight === null || idea.projectId === null) return;`
  2. `setErrorMessage(null)`
  3. `await orchdCreateTask(projectId, null, insight.title, insight.body, null, "insight",
     insight.id, [])` — создаёт задачу (source=insight).
  4. `await orchdSetIdeaLifecycle(idea.id, "specced")` — флипает идею researching→specced.
  5. `await refreshTasks(projectId)`; `await refreshIdeas()`
  6. `showToast("Задача добавлена в backlog")`; `onClose()`
  catch → `setErrorMessage`+`showToast`, диалог остаётся открыт.
  `set_idea_lifecycle` (`persistence.rs:1855-1879`) — БЕЗ state-machine (any→any), падает только на
  unknown-id/archived-project/Io — то есть детерминированно не падает, только на инфра-сбое.
- **Перечисление состояний частичного отказа:**
  - **(A) CreateTask падает:** поймано, inline+toast, диалог открыт. Задача НЕ создана, идея
    `researching`, insight `accepted`. Ретрай «В backlog» безопасен (кроме случая, когда задача на
    сервере создалась, а ответ потерян — тогда ретрай даст дубликат; неидемпотентно).
  - **(B) CreateTask прошёл, SetIdeaLifecycle упал (главный десинк):** задача СОЗДАНА (в backlog,
    source=insight), но идея осталась `researching` (НЕ specced). `refreshTasks`/`refreshIdeas` (шаг 5)
    пропущены — но сервер уже вещал `TasksChanged` из CreateTask → App.tsx (`:202`) вызывает
    `refreshTasks` → задача в табе «Задачи» появится. `IdeasChanged` не вещается (флип не прошёл) →
    идея честно остаётся `researching` и в сторе, и на сервере. Диалог открыт, insight всё ещё
    `accepted` → кнопка «В backlog» снова активна → повторный клик **ПЕРЕзапускает CreateTask** →
    **дублирующая задача**, затем ретрай флипа. Если пользователь сдаётся и закрывает — идея навсегда
    застряла `researching` при существующей specced-задаче (десинк намерение↔реальность).
- **Двойной сабмит (P-19):** guard от in-flight нет. `backlogBlocked` в ходе await не меняется
  (`insight.status` остаётся "accepted") → быстрый двойной клик = 2× CreateTask = 2 задачи (+2
  идемпотентных флипа). То же для «Создать» (`handleCreate`): до резолва `insight===null`, второй
  клик = второй insight.
- **Обработка ошибок:** честная (inline+toast), но БЕЗ компенсации и БЕЗ идемпотентности. Порядок
  (сначала CreateTask — ценный необратимый артефакт, затем флип) защитим: при обратном порядке был бы
  «идея specced без задачи». Тест «once accepted «В backlog» fires orchdCreateTask then
  orchdSetIdeaLifecycle(specced), then closes» мокает ОБА успешными — **частичный отказ (B) не
  покрыт тестом**.
- **Логи:** серверный `TasksChanged{project_id}` из CreateTask; на упавшем SetIdeaLifecycle — FE
  toast. Пер-верб tracing нет (B-04).
- **Что видит пользователь:** в норме — toast + идея `specced`. В сбое (B) — toast с ошибкой флипа,
  диалог открыт, задача уже в backlog, идея всё ещё `researching`; повторная попытка молча плодит
  дубли задач.
- **Дельта от ожидания:** сценарий ждёт «идея specced». При частичном сбое — идея застревает
  `researching` + осиротевшая/дублируемая задача. Триггер требует инфра-сбоя ровно между двумя
  await (падение/дисконнект orchd), т.к. `set_idea_lifecycle` детерминированно не падает.
- **Действие:** BL-кандидат (Important): (1) in-flight-guard от двойного сабмита в
  `handleBacklog`/`handleCreate` (P-19); (2) идемпотентность/ретрай-безопасность формирования задачи
  (напр. не пере-создавать задачу при ретрае после успешного CreateTask — трекать созданный taskId в
  локальном state, ретраить только флип); (3) при десинке — явная нота «задача создана, но статус
  идеи не обновлён — повторите смену статуса», а не немой ретрай всего блока.

# Эпик E — Идеи (гипотезы). Результаты инвестигейта (READ-ONLY, v0.7.0)

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть |
|---|---|---|---|
| E-01 | ✅ OK | — | ⌘K happy: honest toast «идея сохранена», роутинг null→инбокс / id→проект корректен, драфт сохранён при отказе. Минус: нет фидбека, КУДА улетела идея; двойной Enter → дубль (см. E-08). |
| E-02 | ✅ OK | — | orchdDown: кнопка disabled + inline-note `оркестратор недоступен` + honest-guard в handleSubmit. Три уровня защиты, все под тестом. |
| E-03 | ✅ OK | — | ⌘K блокируется в input/textarea/.xterm И под обоими mandatory-UpgradeDialog (daemon+orchd). Все 5 блокировок покрыты тестами. |
| E-04 | ✅ OK | — | «привязать к проекту»: orchdSetIdeaProject + refreshIdeas, try/catch→toast, кнопка disabled при orchdDown/пустом select. |
| E-05 | 🟡 UX-GAP | Important | P-27 подтверждён: правки title/body НЕ ревертятся при отказе (GoalTree ревертит). Стейл-значение висит и НЕ самозалечивается пушем. Lifecycle/delete — ОК. |
| E-06 | ✅ OK | — | Spawn happy-path (порядок цепочки, upsert workspace, toast) + отмена пикера = no-op, под тестами. |
| E-07 | 🟡 UX-GAP | Important | P-09: обрыв после createProject → осиротевший проект+workspace, компенсации нет. НО: push `ProjectsChanged` делает проект видимым, inline-ошибка честна, recovery возможен. Untested. |
| E-08 | 🟡 UX-GAP | Important | P-19: busy-guard'а нет НИГДЕ. ⌘K/create double = 2 идеи (Minor); SpawnProject double = 2 проекта+workspace+сирота (Important, гейтится OS-пикером). |

---

## E-01 — ⌘K happy: title (+body, +проект) → Enter

- **Вердикт:** ✅ OK
- **Проверено:** `QuickCapture.tsx:194-204` (handleSubmit) → `orchd.ts:158-164` (orchdCreateIdea) → `socket_server.rs:989-999` (CreateIdea → `respond_idea` бродкастит `IdeasChanged`) → `App.tsx:200` (`onOrchdIdeasChanged → refreshIdeas`). Тест `QuickCapture.test.tsx:138-200` (happy, роутинг проекта, Enter). Прогон: `npx vitest run` → **15 passed**.
- **Обработка ошибок:** есть и честная. Пустой/whitespace title или orchdDown → ранний `return` (never a doomed send, `QuickCapture.tsx:196`). Реджект → `showToast(describeOrchdError(e))`, оверлей НЕ закрывается (`close()` только после успешного await) → драфт (title/body/projectId) сохранён (тест :230-247). Совпадает с ожиданием каталога «Отказ → toast, драфт сохранён».
- **Логи:** фронт — структурного лога нет, только toast (соответствует конвенции проекта). Бэкенд-арм CreateIdea (`socket_server.rs:989`) — БЕЗ tracing (систем. B-04: во всём socket_server.rs всего 9 tracing-вызовов, на idea-армах — 0). Секретов не логируется (нечего). Открытый вопрос O-6.
- **Что видит пользователь:** оверлей закрывается, toast «идея сохранена». Роутинг: select «без проекта» (пустая строка) → `null` → orphan-инбокс; выбранный проект → его таб «Идеи» (`orchdCreateIdea(projectId === "" ? null : projectId, …)` :198 — `""` никогда не уходит на провод).
- **Дельта от ожидания:** минорная — идея сохраняется, но экран НЕ навигируется к месту, где она осела; при захвате из проекта A идеи в проект B / инбокс пользователь её в текущем view не увидит. Toast «идея сохранена» не сообщает КУДА. Discoverability-нюанс, вероятно by-design для quick-capture.
- **Действие:** ничего (поведение корректно и честно). Опц. док-заметка про «место назначения не подсвечивается».

## E-02 — orchd down: ⌘K

- **Вердикт:** ✅ OK
- **Проверено:** `QuickCapture.tsx:208` (`blocked = orchdDown || title.trim()===""`), `:266-270` (inline-note `quick-capture-orchd-down`), `:284` (`disabled={blocked}`), `:196` (handleSubmit early-return при orchdDown). Тест `QuickCapture.test.tsx:213-228` — disabled + note + wrapper НЕ вызван.
- **Обработка ошибок:** это и есть honest-degradation путь (spec §11 «never a doomed send»). Три слоя: disabled-кнопка, inline-note, guard в handleSubmit — даже если кнопку кликнуть программно, вызова не будет.
- **Логи:** н/д (round-trip не происходит).
- **Что видит пользователь:** оверлей открывается (⌘K не гейтится orchdDown — это read/UI), поле ввода живо, но кнопка «Сохранить» dim/disabled и снизу серо-акцентная заметка «оркестратор недоступен» (не amber — amber зарезервирован под «нужен ты»).
- **Дельта от ожидания:** нет. Каталог: «Кнопка disabled + inline-note» — оба реализованы и протестированы.
- **Действие:** ничего.

## E-03 — ⌘K заблокирован в инпуте/textarea/.xterm И при открытом mandatory-UpgradeDialog

- **Вердикт:** ✅ OK
- **Проверено:** `QuickCapture.tsx:164-180` (глобальный keydown). `isTypingTarget` (:24-29): INPUT/TEXTAREA по tagName + `.closest(".xterm")` (ловит и скрытую `.xterm-helper-textarea`). Плюс два upgrade-гейта: `s.daemonIncompatible && s.upgradeDialogOpen` (:173) и `s.orchdIncompatible && s.orchdUpgradeDialogOpen` (:174) — оба через `useAppStore.getState()` (live-чтение, не stale-closure). Тесты `QuickCapture.test.tsx:73-128` покрывают ВСЕ пять: input, textarea, xterm, daemon-upgrade, orchd-upgrade.
- **Обработка ошибок:** listener безусловный (empty deps), но каждый keydown заново читает `document.activeElement` и стор — не крадёт `k` из активного поля/терминала и не всплывает над блокирующим диалогом версий.
- **Логи:** н/д.
- **Что видит пользователь:** ⌘K в этих контекстах просто ничего не делает (no preventDefault, keystroke идёт по назначению).
- **Дельта от ожидания:** нет — «все три блокировки работают» подтверждено (фактически 3 категории: typing-target + 2 upgrade-диалога; upgrade-часть UpgradeDialog фокусирует КНОПКУ, не input, поэтому нужен отдельный явный гейт, что и сделано).
- **Действие:** ничего.

## E-04 — идея-сирота → «Привязать к проекту»

- **Вердикт:** ✅ OK
- **Проверено:** `IdeasList.tsx:307-333` (orphan-блок: select + кнопка), `:432-439` (handleAttach → `orchdSetIdeaProject` + `refreshIdeas`, try/catch→toast). Кнопка `disabled={disabled || attachTo === ""}` (:326). Бэкенд `persistence.rs:1826-1851` (set_idea_project) гейтит транзакционно И текущий, И целевой проект (архив→Invariant, неизвестный target→NotFound). Тест `IdeasList.test.tsx:201-216` (happy + refresh) и `:302-317` (orchdDown → disabled).
- **Обработка ошибок:** есть, честная — реджект→`showToast(describeOrchdError(e))`; структурная мутация → явный `refreshIdeas()` (идея переезжает из orphan-view). Клиентского дубль-гейта нет, но re-attach идемпотентен.
- **Логи:** фронт — toast; бэкенд SetIdeaProject-арм (`socket_server.rs:1007`) без tracing (B-04).
- **Что видит пользователь:** выбрал проект → «привязать к проекту» → ряд исчезает из «Без проекта», появляется в проекте (через refresh + `IdeasChanged`-push).
- **Дельта от ожидания:** нет.
- **Действие:** ничего.

## E-05 — идея в проекте: править title/body; сменить lifecycle; удалить

- **Вердикт:** 🟡 UX-GAP
- **Severity:** Important (для title/body-путей); lifecycle/delete — OK.
- **Проверено:** `IdeasList.tsx:217-229` (IdeaRow.commitTitle/commitBody), `:398-412` (handleTitleCommit/handleBodyCommit → orchdUpdateIdea, try/catch→toast, **возвращают void**). Сравнение с эталоном `GoalTree.tsx:189-198`: `commit()` получает `ok: boolean` от `onTitleCommit` и `if (!ok) setTitle(goal.title)` — **ревертит**. В IdeaRow реверта нет.
- **P-27 (подтверждён):** после отказа сохранения title/body локальный `title`/`body`-стейт остаётся = отредактированному значению. `useEffect(…,[idea.title])` (`IdeasList.tsx:210-215`) сработает только если `idea.title` в сторе ИЗМЕНИТСЯ — а при неудаче он прежний, поэтому даже последующий `refreshIdeas` (новый объект idea, та же строка title) НЕ ре-триггерит эффект → стейл-значение **не самозалечивается** и висит до размонтирования. GoalTree ревертит немедленно.
- **Обработка ошибок:** реджект честно показывается toast'ом — не проглочено. НО экран после этого показывает несохранённое значение как будто сохранённое (при этом toast — единственный слот, может быть затёрт следующим за <4с, P-21). Lifecycle-select и Удалить читают прямо из стора (`value={idea.lifecycle}`), локального optimistic-стейта нет → «реверт» не нужен, select просто отражает актуальный `idea.lifecycle`; delete — `window.confirm` (:423) + refreshIdeas.
- **Логи:** фронт — toast; бэкенд UpdateIdea/SetIdeaLifecycle/DeleteIdea-армы (`socket_server.rs:1000/1014/1021`) без tracing (B-04).
- **Что видит пользователь:** сменил lifecycle/удалил — честно; отредактировал title/body, сохранение упало — мелькнул toast, но поле продолжает показывать НЕсохранённый текст, создавая ложное ощущение сохранённости; расходится с GoalTree в том же приложении.
- **Дельта от ожидания:** каталог E-05 сам допускает «Отказ → toast, **без реверта**» — т.е. текущее поведение соответствует записанному ожиданию, но это ожидание конфликтует с эталоном GoalTree (P-27 — заявленная несогласованность). Подтверждаю несогласованность как реальную.
- **Действие:** BL-кандидат — привести IdeaRow к контракту GoalRow (onTitleCommit/onBodyCommit возвращают boolean, `if (!ok) revert`). Малый фикс + 2 теста (title-revert, body-revert).

## E-06 — идея-сирота → «Создать проект» (SpawnProjectFromIdea), happy + отмена

- **Вердикт:** ✅ OK
- **Проверено:** `SpawnProjectFromIdea.tsx:66-103` (handleSpawn: pickFolder → createWorkspace + upsertWorkspace → orchdCreateProject → orchdSetIdeaProject → refreshProjects/refreshIdeas → toast «Проект создан из идеи»). Тест `SpawnProjectFromIdea.test.tsx:47-108`: строгий порядок цепочки, имя проекта = idea.title, немедленный upsert workspace, отмена пикера (`dir===null` :78) = no-op. Прогон **5 passed**.
- **Обработка ошибок:** три раздельных catch — pickFolder (:72), createWorkspace (:85, лёгкое sessiond-сообщение), createProject+setIdeaProject (:98, `describeOrchdError`). Ошибка дублируется toast'ом И inline (`spawn-project-error-${id}` :116-120) — устойчиво к затиранию toast'а (P-21).
- **Логи:** фронт — toast+inline; бэкенд CreateProject-арм (`socket_server.rs:858-875`) без tracing, но пишет initial-ruleset + бродкастит ProjectsChanged.
- **Что видит пользователь:** клик → OS-пикер папки → проект появляется в sidebar, идея привязана, toast. Отмена пикера — тихо, ничего не создаётся.
- **Дельта от ожидания:** нет для happy/cancel.
- **Действие:** ничего (для happy-пути). Партиал-фейл — см. E-07.

## E-07 — обрыв цепочки после createProject (осиротевший проект, P-09)

- **Вердикт:** 🟡 UX-GAP
- **Severity:** Important
- **Проверено построчно:** `SpawnProjectFromIdea.tsx:92-102`:
  ```
  try {
    const project = await orchdCreateProject(idea.title, "", [workspaceId]); // (a) успех
    await orchdSetIdeaProject(idea.id, project.id);                          // (b) ПАДАЕТ
    await refreshProjects();  // (c) НЕ достигается
    await refreshIdeas();     // (d) НЕ достигается
    showToast("Проект создан из идеи"); // НЕ достигается
  } catch (e) { setError(describeOrchdError(e)); showToast(...); }  // без компенсации, без refresh
  ```
  createProject и setIdeaProject — в ОДНОМ try, поэтому catch не различает «(a) упал → ничего не создано» и «(b) упал → проект осиротел». Компенсирующего `orchdArchiveProject` нет.
- **Перечень партиал-фейл-состояний цепочки:**
  1. pickFolder упал → toast+inline, ничего не создано. Чисто.
  2. pickFolder отменён (null) → no-op. Чисто.
  3. createWorkspace упал → toast+inline, проект не создан, идея сирота. Чисто (workspace не создан).
  4. createWorkspace ОК, createProject упал → workspace W создан+upsert'нут в стор (осиротевший **workspace**, но это норм-состояние приложения), проекта нет, идея сирота. Утечка: 1 несвязанный workspace. Minor.
  5. **createProject ОК, setIdeaProject упал (ядро P-09):** в БД — проект P (name=idea.title, W привязан, initial-ruleset+md-файл записаны `socket_server.rs:869`); идея I остаётся сиротой (setIdeaProject атомарна, `persistence.rs:1843` — либо привязала целиком, либо нет). Стор: W в сторе; проект P — **появляется** в sidebar, т.к. CreateProject бродкастит `ProjectsChanged` (`socket_server.rs:870`) → `App.tsx:198 refreshProjects()`. Идея I — orphan-view, refreshIdeas в catch не зовётся, но и статус её не менялся.
- **КОРРЕКЦИЯ гипотезы каталога:** тезис «осиротевший проект, юзеру никто не говорит» — частично неверен. Юзер видит (1) honest error-toast, (2) persistent inline-ошибку под кнопкой, (3) новый проект в sidebar (через push). Реальный дефект — не «молчание», а: **(a)** нет компенсации → осиротевший проект (name=idea.title, идея к нему НЕ привязана) остаётся навсегда; **(b)** сообщение вводит в заблуждение — «ошибка», но проект-то создан; **(c)** повторный клик «Создать проект» → ВТОРОЙ пикер → ВТОРОЙ проект+workspace (дубль), т.к. пользователь думает, что первый не создался. Recovery существует: идея всё ещё сирота → select «привязать к проекту» позволяет прицепить её к уже-созданному P (но юзер не знает, что P = только что созданный).
- **Обработка ошибок:** честная (не проглочена), но НЕ атомарная и без rollback; refresh в catch отсутствует (проект всё равно виден через push, идея-статус не тронут).
- **Логи:** фронт — toast+inline; бэкенд — без tracing (B-04): факт «проект создан, но привязка идеи упала» нигде в демоне не фиксируется.
- **Реалистичность триггера:** setIdeaProject при нормальных условиях не падает (идея есть, target активен). Реальный триггер (b): смерть orchd между двумя вызовами, либо гонка (в другом окне идею удалили / проект заархивировали в микрозазор). Редко, но возможно.
- **Тест:** партиал-фейл (a-ОК/b-упал) в `SpawnProjectFromIdea.test.tsx` **НЕ покрыт** (есть только «createProject упал → setIdeaProject не зван», :110-121). Дыра компенсации не под тестом.
- **Дельта от ожидания:** каталог E-07 «3 catch-блока, но компенсации нет» — подтверждено; уточнено, что проект при этом ВИДИМ (push), а не невидим.
- **Действие:** BL-кандидат. Варианты фикса: (1) при отказе setIdeaProject после успешного createProject — компенсировать `orchdArchiveProject(project.id)` в отдельном catch; ИЛИ (2, предпочтительно) честное сообщение «проект создан, но идея не привязана — привяжите вручную» + `refreshProjects()`+`refreshIdeas()` в catch, чтобы состояние сошлось. Плюс тест на этот стейт.

## E-08 — двойной клик/сабмит идеи (P-19)

- **Вердикт:** 🟡 UX-GAP
- **Severity:** Important (взвешено по SpawnProject; ⌘K/create — Minor)
- **Проверено:** busy/submitting/inFlight-guard'а НЕТ ни в одном из трёх сабмит-хендлеров (grep по компонентам: единственные `useState(false)` — `open`/`researchDialogOpen`/`researchExpanded`, не сабмит-флаги). Кнопки `disabled` завязаны только на orchdDown/пустоту title — во время await остаются кликабельны.
  - **QuickCapture** (`:194-204`): двойной Enter/клик — `close()` только ПОСЛЕ await, поэтому второй Enter в окне await → второй `orchdCreateIdea`. **Последствие: 2 идеи (дубль).** Minor.
  - **IdeasList.handleCreate** (`:441-452`): `setCreateTitle("")` после await → второй клик до сброса → второй `orchdCreateIdea`. **Последствие: 2 идеи.** Minor.
  - **SpawnProjectFromIdea.handleSpawn** (`:66-103`): нет флага; каждый клик = свой `pickFolder()`. Если оба пикера отдадут папку — две полные цепочки: 2 workspace + 2 проекта + 2× setIdeaProject (идея уедет на последний проект, первый останется **осиротевшим** — тот же стейт P-09). **Последствие: 2 проекта+workspace + сирота.** Important.
- **Обработка ошибок:** каждая мутация обёрнута в try/catch, но идемпотентности/дедупликации на клиенте нет. Attach/lifecycle-дубли идемпотентны (безвредно); create/spawn-дубли — нет.
- **Логи:** н/д (обычный success-путь, дважды).
- **Что видит пользователь:** для ⌘K/create — две одинаковые строки идей (легко удалить). Для SpawnProject — потенциально два проекта в sidebar, один осиротевший.
- **Дельта от ожидания:** каталог E-08 «Одна мутация» — НЕ гарантируется; фактически 1..2 в зависимости от тайминга.
- **Что не удалось проверить:** практическую реализуемость двойной цепочки SpawnProject — зависит от поведения нативного OS-пикера папок (Tauri dialog-plugin) при двух конкурентных `open()`. В read-only/тест-окружении (без собранного .app) выполнить два реальных выбора папки нельзя. На УРОВНЕ КОДА guard'а нет — уязвимость существует; практический триггер гейтится модальным пикером (нужно завершить два выбора). Для ⌘K/create гейта нет вообще — дубль воспроизводим тривиально.
- **Действие:** BL-кандидат — добавить in-flight-флаг (`useState<boolean>`), `disabled` во время await, ранний return при повторном входе — во все три хендлера. Приоритет: SpawnProject (последствие тяжелее) > create/⌘K.

---

## Сводка по подозрениям (эпик E)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| P-09 (osiротевший проект после обрыва spawn) | 🟡 подтверждён, но смягчён | E-07 — проект ВИДИМ через ProjectsChanged-push; дефект = нет компенсации + вводящее в заблуждение сообщение + дубль при retry; untested |
| P-19 (двойной сабмит) | 🟡 подтверждён | E-08 — guard'а нет нигде; ⌘K/create→2 идеи (Minor), SpawnProject→2 проекта+сирота (Important) |
| P-27 (IdeasList не ревертит title/body) | 🟡 подтверждён | E-05 — IdeaRow без реверта vs GoalRow `if(!ok) setTitle(...)`; не самозалечивается |
| B-04 (нет per-verb tracing) | подтверждён (систем.) | idea/project-армы socket_server.rs — 0 tracing; всего в файле 9 |

**Тесты:** `npx vitest run QuickCapture/IdeasList/SpawnProjectFromIdea` → **40 passed**. Партиал-фейл spawn (E-07) и двойной сабмит (E-08) — не покрыты. P-27 (revert) — не покрыт.

## Волна 2 — эпики C, A, H (2026-07-16)

# Эпик C — Проект (C-01…C-09). Результаты инвестигейта

Репо: `/Users/sshlg/DATA/builder-pro-ai` (main, v0.7.0). READ-ONLY.
Модель: opus. Пути прослежены UI-контрол → ipc → command → wire → dispatch → persistence/export.
Тесты (существующие, не менял): `cargo test -p bpa-orchd --lib -- create_project_creates_strategic_goal_and_ruleset_row remove_project_workspace_last_link_is_invariant archive_project_sets_status_archived archived_project_blocks_archive_project_again import_task_id_collision_is_conflict_and_rolls_back_everything create_project_workspace_linked_to_another_project_is_conflict` → **6 passed**.

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть |
|---|---|---|---|
| C-01 | ✅ OK | — | `create_project` в одной tx сеет strategic-цель «Стратегическая цель» + ruleset-строку + entity_ref граф-узел; toast «Проект создан», диалог закрыт, sidebar-группа появляется |
| C-02 | 🟡 UX-GAP | Minor | **P-06:** «+ проект» (sidebar) и submit CreateProjectDialog НЕ гейтятся `orchdDown`; но при down `orchdCreateProject` быстро реджектится `disconnected` → «оркестратор недоступен» inline-alert+toast, диалог открыт (честно, но негейчено — рассинхрон с остальным app) |
| C-03 | 🟡 UX-GAP | Minor | Главный гвард ✅ («нужен хотя бы один workspace» + disabled submit). **P-17:** вложенный «+ создать workspace» на отказе показывает сырой `e.message` (с generic-fallback), мимо `describeOrchdError`/локализации |
| C-04 | ✅ OK | — (📄 O-2) | Гонка «workspace привязан в другом окне» → `Conflict` → «конфликт: workspace \<id\> is already linked to a project» inline+toast, диалог открыт; читаемо, но хвост сообщения — англ. |
| C-05 | 🟡 UX-GAP | Minor | **F-5/BL-61:** отвязка последнего workspace → «недопустимая операция: **cannot remove the project's last workspace link**» — юзер видит англ. Invariant-текст (locked-копия «у проекта должен остаться workspace» кодом НЕ производится). **P-06:** весь таб «Обзор» негейчен по `orchdDown` (только баннер), но каждая мутация → честный toast + `refreshProjects` |
| C-06 | 🟡 UX-GAP | Minor | **P-28:** «Скопировать JSON» — один catch сливает отказ экспорта (orchd `Io`) и отказ clipboard (DOMException); clipboard-ошибка не распознаётся → «неизвестная ошибка оркестратора» (врёт про виновника). «Сохранить в файл…» — честно |
| C-07 | 🔴 BUG | Minor | **B-07 ПОДТВЕРЖДЁН:** `import_ruleset` пишет .md (`write_atomic`, export.rs:335) ДО `insert_ruleset_raw` (339) и ДО `tx.commit()` (418) → при коллизии позже по бандлу DB откатывается, а .md-файл раннего бандла ПЕРЕЖИВАЕТ rollback (противоречит doc-коммент «nothing survives» + spec §8). Латентно/self-healing/контейнед → Minor. Остальные import-ошибки честны |
| C-08 | 📄 DOC-GAP | Minor | **F-8 ПОДТВЕРЖДЁН:** verb `ArchiveProject` полностью прошит (ipc:96 + socket:887 + persistence:1368 + тесты), но НИ ОДИН UI-контрол его не зовёт (grep `src/components/**`+`App.tsx` = 0 вызовов, нет лейбла «Архивировать»). Архивирование проекта UI-НЕДОСТИЖИМО в v1. **BL-53:** разархивирования нет (un-archive verb отсутствует; повторный archive → `Invariant`) |
| C-09 | ✅ OK | — | Чип «workspace недоступен» (ProjectPanel:335-337) + [Отвязать] (339-346) есть и работает; ref «unresolvable» = id отсутствует в sessiond-слайсе `workspaces` (soft-ref, не FK); покрыт тестом `detach-ghost-ws` |

**Итог по эпику:** 3×✅ OK · 5×🟡 UX-GAP (все Minor) · 1×🔴 BUG (Minor, латентный) · внутри — 1×📄 DOC-GAP (C-08/F-8). Ключевые подтверждения: F-8 (архив-контрола нет), F-5/BL-61 (англ. Invariant-текст), P-06 (весь «Обзор» + CreateProject негейчены), B-07 (md переживает rollback), P-28 (слитые причины отказа).

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| F-8 (ArchiveProject verb без UI-контрола) | 📄 **ПОДТВЕРЖДЁН** | C-08 |
| BL-53 (нет разархивирования) | ✅ подтверждён (кода un-archive нет) | C-08 |
| P-06 (нет orchdDown-гейта: CreateProject submit + весь «Обзор») | 🟡 **ПОДТВЕРЖДЁН** | C-02, C-05 |
| F-5/BL-61 (англ. Invariant-текст вместо locked-копии) | 🟡 **ПОДТВЕРЖДЁН** | C-05 |
| P-17 (сырой message в nested create-workspace) | 🟡 подтверждён | C-03 |
| P-28 (слитые export vs clipboard отказы) | 🟡 подтверждён | C-06 |
| B-07 (md-файл переживает rollback) | 🔴 подтверждён (латентный) | C-07 |

---

## Результаты

### C-01 — «+ проект»: имя + ≥1 workspace → создать (strategic-цель автосоздана?)

- **Вердикт:** ✅ OK.
- **Проверено:** `WorkspaceSidebar.tsx:257-274` («+ проект» → `setShowCreateDialog(true)`) → `CreateProjectDialog.tsx:230-242 handleSubmit` → `orchdCreateProject(name.trim(), description, selectedIds)` → `orchd_create_project` → `socket_server` dispatch → `persistence.rs:1276 create_project`. Внутри ОДНОЙ tx: insert project → insert `project_workspace` (ord 0..) → insert strategic `goal` (`STRATEGIC_GOAL_TITLE = "Стратегическая цель"`, persistence.rs:604; строка вставки 1306-1313) → `crate::graph::seed_strategic_entity_ref` (1316-1319, S4 §5 D6) → ruleset-строка. Тест `create_project_creates_strategic_goal_and_ruleset_row` → **ok**.
- **Обработка ошибок:** есть, честная. `handleSubmit` guard `blocked || name.trim()===""` (231); success → `showToast("Проект создан")` + `onClose()` (235-236); fail → `describeOrchdError` → `setCreateError` inline `role="alert"` (323-326) + toast, диалог открыт.
- **Логи:** пер-верб tracing нет (B-04-класс, системное). Секретов нет.
- **Что видит пользователь:** проект создан, toast «Проект создан», диалог закрыт; sidebar-группа появляется (push `orchd://projects-changed` + пере-рендер по стору). Открыв таб «Цели», сразу видит корневую «Стратегическая цель» (D-01 смежно — цель реально автосоздаётся в create_project, не отдельным шагом).
- **Действие:** ничего.

### C-02 — orchd down: «+ проект» → заполнить → submit (P-06)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-06 подтверждён.)
- **Проверено:** «+ проект» кнопка (`WorkspaceSidebar.tsx:257-274`) — `onClick` без `disabled={orchdDown}`. `CreateProjectDialog.handleSubmit` (230) — НЕТ проверки `orchdDown` (в компоненте `orchdDown` вообще не читается). Submit disabled только на `blocked || name.trim()===""` (335).
- **Что реально происходит при down:** при `orchdDown===true` соединение сброшено → `orchdCreateProject`.invoke реджектится `CommandError{kind:"disconnected"}` БЫСТРО (не 30-сек висяк: клиент считает себя отключённым) → catch → `describeOrchdError` → «оркестратор недоступен» → inline `role="alert"` (load-bearing, переживает clobber toast-очереди) + toast, диалог остаётся открыт.
- **Обработка ошибок:** есть, честная — но реактивная, а не проактивный гейт. Каталог C-02 допускает «disabled ИЛИ честная ошибка» → честная ошибка присутствует. Дельта: остальной app disable-ит мутирующие контролы при down; здесь — нет (несогласованность UX, юзер тратит клик на обречённую операцию, хотя баннер «оркестратор недоступен» уже виден в панели проекта; в sidebar баннера нет вовсе).
- **Логи:** FE-toast; секретов нет.
- **Действие:** BL/фикс (Minor): гейтить submit CreateProjectDialog (и/или кнопку «+ проект») на `orchdDown` — зеркально ConnectDialog/остальным диалогам.

### C-03 — Диалог проекта: 0 workspaces выбрано (+ P-17 nested create)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (Главный путь ✅; P-17 подтверждён.)
- **Проверено:** `blocked = selectedIds.length === 0` (CreateProjectDialog.tsx:197). При blocked — inline `role="alert"` `BLOCKED_TEXT = "нужен хотя бы один workspace"` (25, 311-315) + submit `disabled` (335) + `handleSubmit` ранний `return` (231). Всё корректно.
- **P-17 (nested «+ создать workspace»):** `handleCreateWorkspace` (216-228) → `pickFolder` → `createWorkspace(basename(dir), dir)` (sessiond-путь, НЕ `orchd_*`). Catch: `const message = e instanceof Error ? e.message : "не удалось создать workspace"` (224) → сырой `e.message` (или generic-fallback), в обход `describeOrchdError`/локализованного маппера. Осознанно по doc-комменту (174-176: «sessiond createWorkspace failure … lighter, non-`describeOrchdError` message»), но это НЕлокализованная/техническая поверхность (P-17 фактически подтверждён).
- **Что видит пользователь:** без workspace — красная строка «нужен хотя бы один workspace», submit серый. При отказе создания вложенного workspace — inline-alert + toast с сырым/generic текстом.
- **Действие:** BL (Minor): прогнать nested-create-workspace ошибку через какой-либо локализованный маппер (или хотя бы `describeCommandError`), чтобы юзер не видел сырой сессионд-message.

### C-04 — Диалог проекта: workspace, уже привязанный к другому проекту (гонка → Conflict)

- **Вердикт:** ✅ OK (📄 Minor: англ. хвост, O-2).
- **Проверено:** UI прячет привязанные workspace (`unlinked = filter(!linkedIds.has(w.id))`, CreateProjectDialog.tsx:194-196) — unlinked-only. Гонка (привязали в другом окне между открытием и submit): `create_project` вставляет `project_workspace` (persistence.rs:1298-1303); `project_workspace.workspace_id` — UNIQUE (263-267) → `map_workspace_conflict` (662-671) → `Conflict("workspace {workspace_id} is already linked to a project")`, откат всей tx. `describeOrchdError` → «конфликт: workspace \<id\> is already linked to a project» → inline+toast, диалог открыт. Тест `create_project_workspace_linked_to_another_project_is_conflict` → **ok**.
- **Обработка ошибок:** есть, честная; вся транзакция откатывается (проект-строка не остаётся).
- **Что видит пользователь:** читаемое «конфликт: …», но `{message}`-хвост английский + сырой uuid workspace (не имя) — минорная читаемость (O-2).
- **Действие:** ничего по коду; при желании — Minor: локализовать/подставлять имя workspace вместо id в сообщение конфликта.

### C-05 — Таб «Обзор»: привязать/отвязать workspace (F-5/BL-61 + P-06)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (F-5/BL-61 + P-06 подтверждены.)
- **Проверено (F-5/BL-61):** «Отвязать» (`ProjectPanel.tsx:339-346 handleDetachWorkspace:204-211`) → `orchdRemoveProjectWorkspace` → `persistence.rs:1434 remove_project_workspace`: если `is_linked>0 && total<=1` → `OrchdPersistError::Invariant("cannot remove the project's last workspace link")` (1452-1456). Обёртка `describeOrchdError`: `case "Invariant": return "недопустимая операция: ${message}"` (orchd.ts:769-770) → пользователь видит **«недопустимая операция: cannot remove the project's last workspace link»** — англ. Invariant-текст. Locked-копия из backlog.md:71 «у проекта должен остаться workspace» кодом НЕ производится (F-5). Тест `remove_project_workspace_last_link_is_invariant` → **ok**.
- **Проверено (P-06):** ВЕСЬ таб «Обзор» негейчен: `grep orchdDown|disabled` по `ProjectPanel.tsx` → только рендер баннера (283), НИ ОДНОГО `disabled` на detach/add-select/copy/export/import. При down каждая мутация → catch → `showToast(describeOrchdError)` = «оркестратор недоступен» (честно, но реактивно). Add: `handleAddWorkspace` (213-223) → `orchdAddProjectWorkspace` + `refreshProjects` + сброс select. Push/refresh: каждая мутация, меняющая `workspaceIds`, явно `refreshProjects()` (207, 218) → sidebar сводится.
- **Что видит пользователь:** ряд появился/исчез, sidebar обновлён; при последнем workspace — англ. Invariant-toast; при down — «оркестратор недоступен»-toast (контролы кликабельны).
- **Действие:** (1) BL/фикс (Minor): гейтить контролы «Обзора» на `orchdDown` (P-06). (2) Док/локализация (F-5/O-2): либо маппить last-workspace Invariant в русскую копию, либо принять англ. Invariant-хвост как политику.

### C-06 — Таб «Обзор»: «Скопировать JSON» / «Сохранить в файл…» (P-28)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-28 подтверждён.)
- **Проверено:** `handleCopyJson` (ProjectPanel.tsx:225-233): `try { json = orchdExportProject(); navigator.clipboard.writeText(json); showToast("JSON скопирован") } catch (e) { showToast(describeOrchdError(e)) }`. Один catch на ДВА разных источника: (1) отказ экспорта — orchd `Io` (>16MiB и пр.) → `describeOrchdError` → «ошибка сервиса: …» (честно); (2) отказ clipboard — `navigator.clipboard.writeText` бросает DOMException (нет `kind`) → `describeOrchdError` не распознаёт → «неизвестная ошибка оркестратора» — **врёт про виновника** (orchd ни при чём, это браузер/permission). `handleExportToFile` (235-244) — отдельный путь, `Io` честно → «ошибка сервиса».
- **Что видит пользователь:** при clipboard-отказе — сообщение, обвиняющее оркестратор, вместо «не удалось скопировать в буфер обмена».
- **Действие:** BL (Minor): развести две причины — обернуть `navigator.clipboard.writeText` в свой try и показывать честное «не удалось скопировать в буфер»; `describeOrchdError` — только для отказа экспорта.

### C-07 — Таб «Обзор»: «Импорт из файла…» → .json (B-07 rollback)

- **Вердикт:** 🔴 BUG. **Severity: Minor** (латентный, self-healing, не user-visible). B-07 ПОДТВЕРЖДЁН статически + существующим тестом (который дефект НЕ ловит).
- **Проверено (B-07):** `handleImportFile` (ProjectPanel.tsx:258-272) → `orchdImportFromFile` → `export.rs:429 import_bundle` → `import_project_bundles` (384-420). Порядок: `tx` открыт (392), `defer_foreign_keys=ON` (399), цикл по `project_bundles` (402-404) → `import_one_project` → внутри бандла порядок project→goals→ideas→insights→tasks→**ruleset** (353-373). `import_ruleset` (314-340): `ruleset_files::write_atomic(&effective_path, content)` (335) — реальная запись .md на диск (`std::fs::write` + `rename`, ruleset_files.rs:39-47) — **ДО** `insert_ruleset_raw` (339) и **ДО** `tx.commit()` (418). При коллизии PK/UNIQUE позже (второй project-бандл в whole-store импорте / global_ruleset / orphan-insert / либо ruleset-id-коллизия в том же бандле) → `?` роняет `tx` без commit → **DB полностью откатывается, но уже записанный .md раннего бандла ОСТАЁТСЯ на диске**. Это противоречит doc-комменту `import_project_bundles` (380-383: «nothing survives — tx is simply dropped without commit()») и spec §8 атомарности.
- **Границы урона (почему Minor):** путь fail-closed валидирован (`resolve_ruleset_write_path` реджектит `..`, `validate_path_within` кэнонизирует parent — export.rs:310-334) → эскейпа из `app_support` нет; путь scope-детерминированный (`project-<id>.md` / global default) → повторный УСПЕШНЫЙ импорт перезапишет его атомарно; без соответствующей DB-строки осиротевший файл не всплывает ни в одном `RuleSetView`. Итог: латентная нецелостность (FS-side-effect переживает DB-rollback), НЕ видимая пользователю и самозаживающая.
- **Тест-доказательство:** `import_task_id_collision_is_conflict_and_rolls_back_everything` (export.rs:693-746) → **ok**, но ассертит ТОЛЬКО `table_counts` (DB rollback), .md-файл не проверяет (грепы `.exists()/read_dir/orphan` по export.rs — ноль FS-ассертов). Причём в ЭТОМ тесте (single-project bundle) task-коллизия (368) опережает ruleset-запись (372) → орфан не создаётся; репро — именно multi-bundle whole-store (`orchd_export_all` даёт ключ `projects`) с коллизией в позднем бандле. `build_fixture` (523) реально сеет ruleset с md_content (upsert_ruleset, 66-70) → путь записи в импортах задействуется.
- **Остальные import-ошибки честны:** Conflict→«конфликт: … already exists», Validation (малформ/`bundleFormat`≠1)→«неверные данные: …» (export.rs:434-472), 0 json → «Нет .json файлов в выбранной папке» (ProjectPanel:402-403), success → покатегорийные счётчики (262-265) + `refreshProjects`.
- **Действие:** BL (🔴 Minor): либо отложить ВСЕ `write_atomic` до после `tx.commit()` (собрать записи, применить на success), либо чистить записанные .md при rollback (`?`-guard), либо честно ослабить doc/spec-инвариант. Добавить FS-ассерт в collision-тест (multi-bundle).

### C-08 — Проект существует: Архивировать проект (F-8 / BL-53)

- **Вердикт:** 📄 DOC-GAP. **Severity: Minor.** F-8 ПОДТВЕРЖДЁН.
- **Проверено (нет UI-контрола):** verb `ArchiveProject` полностью прошит — `orchdArchiveProject` (`ipc/orchd.ts:96` → `invoke("orchd_archive_project")`), wire `socket_server.rs:887 OrchdRequest::ArchiveProject`, `persistence.rs:1368 archive_project`, юнит-тест `orchd.test.ts:124`. НО: `grep -rn "orchdArchiveProject|ArchiveProject|Архивир" src/components/ src/App.tsx` → **0 вызовов**; единственные вхождения `orchdArchiveProject` во всём `src/` — сам ipc-враппер (orchd.ts:96) и его юнит-тест. Лейбла «Архивировать»/«Архив проекта» нет нигде. `InsightsList`-«archive» вхождения — про АРХИВ ИНСАЙТА, не проекта. ⇒ **архивирование проекта UI-НЕДОСТИЖИМО в v1** (verb есть, кнопки нет).
- **Проверено (BL-53 — нет разархивирования):** `archive_project` (1368-1378) — one-way: `ensure_project_active(&tx, id)` до `UPDATE status='archived'`; un-archive verb в кодовой базе отсутствует. Повторный archive архивного → `Invariant` (тест `archived_project_blocks_archive_project_again` → **ok**). Бэкенд корректно делает архивный проект read-only: все мутации → `Invariant` через `ensure_project_active` (15+ тестов `archived_project_blocks_*`, напр. `archived_project_blocks_update_project/_create_goal/_add_project_workspace/_remove_project_workspace` → ok). Тест `archive_project_sets_status_archived` → **ok**.
- **Что видит пользователь:** никак не может архивировать проект (нет контрола). Если бы контрол появился — архив был бы необратим (нет un-archive), архивный проект — только чтение (списки/экспорт работают: `list_goals`/`get_project` без archived-guard, 1145/1395).
- **Дельта от ожидания:** каталог C-08 «Ожидаемо: ???» + F-8 «кнопки архива, похоже, нет» — подтверждено: нет. Это осознанный gap (открытый вопрос O-3): verb-плюс-нет-UI = capability прошита, но не выставлена в v1.
- **Действие:** 📄 DOC + владелец (O-3): либо задокументировать «архив проекта отложен из v1 UI» (и, при выставлении, — обязательно un-archive-путь, BL-53), либо завести BL на UI-контрол «Архивировать» с confirm + честной нотой о необратимости.

### C-09 — Проект с недоступным workspace: открыть «Обзор» (unresolvable soft-ref)

- **Вердикт:** ✅ OK.
- **Проверено:** `ProjectPanel.tsx:328-349` — для каждого `project.workspaceIds`: `const ws = workspaces[wsId]`; если `ws` резолвится → имя (333), иначе → чип `data-testid="project-workspace-unresolved-{wsId}"` **«workspace недоступен»** (335-337, стиль `chipStyle` — statusExited-рамка) + кнопка **[Отвязать]** (339-346 → `handleDetachWorkspace(wsId)`), а не тихий дроп ряда. `workspaces` — sessiond-слайс (soft-ref join, spec §10); ref «unresolvable», когда id отсутствует в слайсе (workspace удалён/никогда не существовал — soft ref, не FK). Покрыт тестом (`ProjectPanel.test.tsx:245` кликает `project-workspace-detach-ghost-ws`).
- **Обработка ошибок:** detach-путь честный (try/catch→toast + refreshProjects). Edge: отвязка недоступного, если он ПОСЛЕДНИЙ → `Invariant` (honest toast, C-05).
- **Что видит пользователь:** чип «workspace недоступен» + рабочая [Отвязать] — может почистить висячий ref.
- **Действие:** ничего.

---

## Не удалось проверить рантаймом

1. **B-07 (C-07) физический орфан .md после rollback** — вердикт построен на статик-трейсе (`write_atomic` до `tx.commit`) + факте, что существующий collision-тест не проверяет файл; НОВЫЙ репро-тест не писал (read-only мандат). Multi-bundle whole-store репро выведён из порядка цикла `import_project_bundles` + write-before-commit; уверенность высокая, но не исполнен как свежий репро.
2. **C-02/C-05 рантайм-поведение негейченных контролов при orchd down** (что реджект быстрый `disconnected`, а не 30-сек висяк) — выведено из connection-модели orchd_client (документирована в предыдущем анализе F-03), здесь повторно не исполнялось.
3. **C-04 реальная гонка двух окон** — single-instance-модель (K-05) не проверял; Conflict-путь доказан юнит-тестом create_project, не мульти-оконным репро.

# Эпик A — Первый запуск и здоровье демонов: инвестигейт A-01..A-10 (READ-ONLY, v0.7.0)

> Пути прослежены: launchd bring-up (`launchd.rs` → `lib.rs::ensure_daemon_running`/`bring_up_daemon`/`bring_up_orchd`)
> → connect (`socket_client.rs`/`orchd_client.rs`) → события (`broker`) → UI (`App.tsx`, баннеры, `UpgradeDialog`).
> Тесты прогнаны: `npx vitest run UpgradeDialog/DaemonBanner/OrchdDownBanner/HomeView/WorkspaceSidebar` → **50 passed**.
> Rust-пути (launchd/boot/persistence/reconnect) прослежены статически + сверены с существующими юнит-тестами в этих же файлах.

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть |
|---|---|---|---|
| A-01 | 🟡 UX-GAP | Important | Окно открывается сразу, оба демона поднимаются параллельно, паники нет. НО жёсткий отказ install/bootstrap/kickstart (TCC/права/нет бинаря) → generic `daemon://disconnected` («…reconnecting…») / `orchd://down` — типизированный `LaunchdError` только логируется, юзеру не показан; «reconnecting…» вводит в заблуждение при перманентном отказе; онбординга нет (F-6) |
| A-02 | 🟡 UX-GAP | Minor | 0 workspaces/0 projects: sidebar показывает голый заголовок «Без проекта» (P-11) но CTA «+ Add workspace»/«+ проект» есть; Home (дефолт-view) при 0 workspaces показывает ТОЛЬКО «Нет активных сессий.» без CTA (протестировано). Нет empty-state-текста и онбординга (F-6) |
| A-03 | 🟡 UX-GAP | Minor | Reconnect-машинерия честна и полна: disconnect→in-flight fail Disconnected→баннер→bounded backoff (100ms→5s)→reconnect→re-attach всех сессий (видимая eager, скрытые lazy, replay-only). НО ввод в мёртвый PTY: `term.onData→writeStdin` fire-and-forget, без `.catch`, без гейта → символы молча теряются (нет локального эха, нет фидбека), потеряны навсегда (свежий shell при reconnect) |
| A-04 | 🟡 UX-GAP | Minor | OrchdDownBanner + [Повторить] есть; чтения живут; гейт мутаций по `orchdDown` работает для дисциплинированных контролов. Гэпы: [Повторить] даёт НОЛЬ фидбека (нет busy/disabled/спиннера; команда возвращает Ok мгновенно, реконнект — только через orchd://down|up); P-06-дыры реальны (submit CreateProjectDialog и attach-select sidebar НЕ гейтятся) — но кликаются и падают в честный toast «оркестратор недоступен», не молча |
| A-05 | ✅ OK | — | Несовместимый sessiond → UpgradeDialog с локед-копией про N живых сессий (hydrated-гейт честен, finding [14]); «Обновить» fire-and-forget + `.catch`→inline-ошибка+retry; рестарт по успеху |
| A-06 | ✅ OK | — | Несовместимый orchd → orchd-вариант UpgradeDialog (локед-копия, БЕЗ предупреждения о сессиях); «Обновить»→`orchdUpgrade`→рестарт; `.catch`→orchd-специфичная inline-ошибка. (Отмена → см. A-10) |
| A-07 | ✅ OK | — | Оба несовместимы → sessiond первым (precedence: `sessiondOpen` проверяется ПЕРВЫМ и выигрывает; `orchdOpen = !sessiondOpen && …`), ровно ОДИН диалог за раз; после разрешения sessiond (upgrade→restart→re-detect ИЛИ Cancel→orchd показывается следующим, тест :303) — orchd. Порядок соблюдён |
| A-08 | 🟡 UX-GAP | Important | Повреждённый orchd.db → `Db::open` карантинит `orchd.db.corrupt-<ts>` + чистит -wal/-shm + создаёт свежую БД, стартует; логирует `warn!`. **UI не сообщает НИЧЕГО** — юзер видит пустой аккаунт без объяснения. Данные не уничтожены (карантин восстановим), но юзеру про это никто не говорит |
| A-09 | 🟡 UX-GAP | Important | Диск/директория недоступна → `open_db_degrading` ловит Err, логирует `error!` «continuing in degraded (in-memory) mode», поднимает in-memory БД. Демон живёт, **НИЧЕГО не персистится, НОЛЬ индикации в UI**. Юзер может отработать целую сессию и потерять 100% при рестарте без предупреждения (граничит с 🔴 silent data-loss) |
| A-10 | 🟡 UX-GAP | Important | «Отмена» закрывает только `*UpgradeDialogOpen`, фатальный `*Incompatible` остаётся. **Возвратный путь ЕСТЬ только для sessiond** (DaemonBanner incompatible-ветка → «Обновить» → reopen). Для **orchd** после Cancel: `orchdIncompatible=true` но `orchdDown=false` → OrchdDownBanner не монтируется, `orchdIncompatible` не читает НИ ОДИН баннер → пусто, диалог не вернуть до рестарта app |

**Итог:** 3×✅ OK · 7×🟡 UX-GAP (5 Important + 2 Minor) · 0×🔴. Две самые ценные находки — A-08/A-09 (молчаливая потеря/неперсистентность данных без индикации) и A-10-orchd (потерянный upgrade-диалог orchd без возвратного пути).

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где |
|---|---|---|
| P-05 ([Повторить] без `.catch`) | 🟡 переквалифицирован | A-04 — не «нет .catch» (команда возвращает Ok сразу, ловить нечего), а «нет busy/feedback-стейта» |
| P-06 (негейченные мутации при orchdDown) | 🟡 подтверждён | A-04 — CreateProjectDialog `blocked=selectedIds.length===0` (без orchdDown); attach-select sidebar без гейта; оба падают в честный toast |
| P-11 (голый заголовок sidebar при 0 ws) | 🟡 подтверждён | A-02 — «Без проекта» рендерится безусловно даже с 0 workspaces |
| F-6 (нет онбординга/первого запуска) | 📄 подтверждён | A-01/A-02 — ни в одном доке, ни в UI |

---

## Результаты

### A-01 — Чистая машина, первый запуск .app

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.**
- **Проверено (полный boot-путь):**
  - `lib.rs::run` → `.setup()` (760-810): строит `Broker`, пре-создаёт orchd slot/status + агент, **спавнит `bring_up_daemon` и `bring_up_orchd` двумя независимыми задачами** на `tauri::async_runtime` и **сразу возвращает `Ok(())`** → окно открывается немедленно, никакого блокирования на launchctl/сети, паники нет ни при каком исходе (док-модуля §"Always-managed AppState").
  - Успех-путь: `ensure_daemon_running` (`lib.rs:261-266` = `install_agent`→`bootstrap`→`kickstart`, все идемпотентны, `launchd.rs`) → `connect_with_retry` (BOOT_CONNECT_ATTEMPTS=8 × 500ms ≈ до 4с) → `Ok(client)` → slot заполнен, `register`/`register_orchd` вешают push/conn. Событий на первый успешный коннект НЕ шлётся (только `daemon://reconnected` на ПОЗДНИЙ reconnect) — начальную гидрацию делает `App.tsx::hydrate(0)` + `refreshProjects()` с retry. Баннеров нет, Home открыт.
  - **Отказ-путь (ключевой вопрос сценария):** любой из трёх шагов падает → `ensure_daemon_running` возвращает `Err(LaunchdError::{Install|Command|Io|DaemonPath})`. В `bring_up_daemon` (434-448): `error!(error=%e, …)` + `emit_disconnected(&app, "could not start background service")` → `daemon://disconnected` → `App` `setDaemonConnected(false)` → **DaemonBanner показывает «Daemon disconnected — reconnecting…»** (красный). Для orchd (`bring_up_orchd:550-555`): `emit_orchd_down("could not start orchd background service")` → **OrchdDownBanner «Оркестратор недоступен» + [Повторить]**. Отказ резолва бинаря (`build_launchd_agent` Err) → `emit_disconnected("could not resolve the background service binary")` — тот же generic баннер.
- **Обработка ошибок:** честная в смысле «не паника, не тихий висяк» (spec §13). НО типизированная причина (`LaunchdError::Install(stderr)` — например «Operation not permitted (TCC)», тест `hard_failure_surfaces_install_error` в `launchd.rs`) **только логируется `error!`, юзеру не показывается**. Юзер видит generic «reconnecting…», что при перманентном отказе (TCC-денайл прав, отсутствие бинаря) **вводит в заблуждение**: приложение действительно крутит hydrate-retry-петлю, но она никогда не преуспеет — а хинта «нужно выдать разрешение / переустановить» нет.
- **Логи:** `installed LaunchAgent plist` (info); при отказе `failed to bring up the launchd-managed daemon` / `…orchd daemon` (error, с `%e`); `emitting daemon://disconnected` / `orchd://down` (warn, reason). Секретов нет.
- **Что видит пользователь:** окно и Home сразу (интерактивность немедленная, реальные данные подтягиваются ~до 4с + hydrate-retry). При жёстком отказе демона — красный баннер «reconnecting…» / «Оркестратор недоступен» без указания причины и без онбординга (F-6: первый запуск не описан ни в одном доке).
- **Дельта от ожидания:** каталог «Отказ install/bootstrap любого демона — что видит юзер?» → видит generic-баннер, а не actionable-сообщение про TCC/права. «Сколько ждать до интерактивности» → мгновенно (окно), реальные списки — до ~4с.
- **Действие:** BL-кандидат (Important): при `ensure_daemon_running`-отказе прокидывать типизированную причину в отдельный actionable-баннер («не удалось запустить фоновый сервис: проверьте разрешения launchctl» вместо «reconnecting…»); онбординг/пустой первый запуск (F-6).

### A-02 — Первый запуск, 0 workspaces / 0 projects

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-11 подтверждён; F-6.)
- **Проверено:**
  - **Sidebar** (`WorkspaceSidebar.tsx`): при пустых `projects`/`workspaces` — `sortedProjects=[]` (нет групп проектов), `unlinkedWorkspaces=[]` (пустой `<ul>`), но заголовок **«Без проекта» рендерится безусловно** (214-226) → голый заголовок (P-11). Кнопки **«+ проект»** (257-274) и **«+ Add workspace»** (275-291) присутствуют ВСЕГДА → discoverable CTA для создания workspace ЕСТЬ. Нет только empty-state-ТЕКСТА («ещё нет workspace — создайте первый»).
  - **HomeView** (`HomeView.tsx:230-247`): при `all.length===0` — блок `home-empty` с «Нет активных сессий.» + кнопка «Открыть {firstWorkspace.name}» **только если `firstWorkspace` существует**. При 0 workspaces `firstWorkspace===undefined` → **НИ ОДНОЙ CTA**, только dim-строка. Подтверждено тестом `HomeView.test.tsx:205` «empty state with zero workspaces shows only the dim sentence (no action)».
  - `HomeGoals` монтируется ниже, но сам ничего не рендерит при 0 активных проектов.
- **Обработка ошибок:** н/д (пустой стейт — не ошибка).
- **Логи:** н/д.
- **Что видит пользователь:** на дефолтном Home — только «0 workspaces · 0 live · 0 waiting» + «Нет активных сессий.», без CTA прямо на экране. Единственный путь создать workspace — кнопка «+ Add workspace» в левом рейле (видима, но не подсвечена как «начни отсюда»). Нет empty-state-подсказки/онбординга (F-6).
- **Дельта от ожидания:** каталог ждал «внятный empty-state с CTA "создай workspace"». Фактически: CTA есть в sidebar (не тупик), но Home-экран без CTA, empty-state-текста нет, P-11 (голый заголовок) реален.
- **Действие:** BL-кандидат (Minor): empty-state-текст в sidebar при 0 workspaces; CTA «создать первый workspace» на Home при 0 workspaces; онбординг (F-6).

### A-03 — Приложение работает → убить bpa-sessiond

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (reconnect/re-attach — ✅; ввод в мёртвый PTY — 🟡.)
- **Проверено (полный reconnect-путь):**
  - Обрыв: `connection_task` (`socket_client.rs:950-977`): `run_connection` вернулся → `shared.live=false` → дренаж `pending` (каждый in-flight → `Err(Disconnected)`, честно, не висит) → `fire_conn(ConnState::Disconnected)` → broker → `daemon://disconnected` → `App.tsx:159` `setDaemonConnected(false)` → **DaemonBanner «Daemon disconnected — reconnecting…»** (красный, `statusExited`).
  - Reconnect: `connect_with_backoff` (`:796-813`) — bounded экспоненциальный backoff `BACKOFF_START=100ms` ×2 до `BACKOFF_CAP=5s`, петля пока не поднимется (Io/TransientHandshake ретраятся внутри; только fatal Incompatible выходит). На успехе (`:980-986`): `live=true` → `fire_conn(Connected)` → `daemon://reconnected`.
  - Re-attach ВСЕХ (`App.tsx:160-181` `onDaemonReconnected`): `manager.resetAllAttachments()` (сбрасывает флаг attach у КАЖДОЙ сессии) → `hydrate(0)` → **видимая** сессия ре-attach eager (`manager.attach(id)`), **скрытые** — lazy при следующем tab-switch (пейн ремоунтится, его эффект зовёт attach). Replay-only (crash убил все shell'ы; scrollback реплеится до последнего flush). Соответствует ожиданию «сессии переприкреплены».
- **Ввод в мёртвый PTY (ключевой вопрос):** `terminal-manager.ts:147-148` — `term.onData((data) => { void writeStdin(sessionId, data); })` — **fire-and-forget, без `.catch`, без гейта на `daemonConnected`**. Пока sessiond мёртв: slot ещё `Some` (клиент реконнектится внутри), `write_stdin` → `client.request(WriteStdin)` → `live=false` → `Err(Disconnected)` → промис реджектится → `void` глотает (unhandled rejection, без фидбека). Локального эха нет (xterm в PTY-режиме не эхоит; эхо даёт shell, которого нет) → **символы молча теряются, экран не реагирует, ввод потерян навсегда** (после reconnect — свежий shell).
- **Обработка ошибок:** reconnect-путь — образцово-честный (spec §13, тесты `socket_client.rs`: `…fires_conn_state`, disconnect→reconnect последовательности). Ввод-путь — тихий проглот.
- **Логи:** `daemon connection lost; reconnecting` (warn); `daemon connect failed; will retry` (warn). Ввод-дроп нигде не логируется.
- **Что видит пользователь:** глобальный красный баннер «reconnecting…»; терминал НЕ дизейблится, набранное не эхоится и молча пропадает; после reconnect — чистый ре-attach со scrollback.
- **Дельта от ожидания:** reconnect/replay соответствуют каталогу. «Что с вводом, набранным в мёртвый PTY» → молча теряется, без per-keystroke-фидбека (только глобальный баннер сигналит, что что-то не так).
- **Действие:** BL-кандидат (Minor): при `!daemonConnected` дизейблить/визуально гасить ввод терминала или показывать «ввод недоступен — переподключение», а не молча ронять `writeStdin`.

### A-04 — Приложение работает → убить bpa-orchd

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-05 переквалифицирован; P-06 подтверждён.)
- **Проверено:**
  - Баннер: `App.tsx:442` `{orchdDown && <OrchdDownBanner/>}`; `orchd://down` → `setOrchdDown(true)`. OrchdDownBanner — красный left-edge + [Повторить] (`OrchdDownBanner.tsx`).
  - **[Повторить]** (`:47-52`): `onClick={() => void orchdReconnect()}` → `orchd_reconnect` (`commands.rs:1928-1939`): `state.orchd.write().take()` (дропает slot) → `spawn(bring_up_orchd(...))` → **возвращает `Ok(())` МГНОВЕННО**. Результат наблюдается ТОЛЬКО через `orchd://down`/`orchd://up`. Т.е. P-05 «без `.catch`» — формально верно, но ловить нечего (Ok сразу); реальный гэп — **нет busy/feedback-стейта**.
  - Гейт: `orchdDown` дизейблит дисциплинированные мутирующие контролы (идеи/research/insight — эпики E/F/G). Чтения (списки, «показать артефакт») живут.
  - **P-06-дыры (подтверждены статически):** `CreateProjectDialog.tsx:197` `blocked = selectedIds.length===0` — **orchdDown НЕ входит** → «Создать» кликабелен; клик → `orchdCreateProject` реджект `Disconnected` → `catch` → `describeOrchdError`→«оркестратор недоступен» inline+toast (не молча). `WorkspaceSidebar.tsx:75-85` `handleAttach` — без orchdDown-гейта, select всегда рендерится → клик → `orchdAddProjectWorkspace` реджект → toast «оркестратор недоступен».
- **[Повторить] при всё ещё мёртвом orchd (ключевой вопрос):** клик → slot дропнут, `bring_up_orchd` спавнится, `ensure_daemon_running` (bootstrap идемпотентен) ок, `connect_orchd_with_retry` (8×500ms≈4с) падает → `emit_orchd_down` → `orchd://down` → `setOrchdDown(true)` (уже true → no-op). **Весь ~4с — НОЛЬ видимых изменений**: кнопка не дизейблится, спиннера нет, баннер стоит. Юзер не понимает, идёт ли попытка.
- **Обработка ошибок:** мутации при down недостижимы (где гейт есть) ИЛИ падают в честный toast (P-06-дыры). Ни silent-no-op, ни ложь. Гэп — отсутствие фидбека у [Повторить].
- **Логи:** `orchd connect failed after bounded retry` (error) на каждой неудачной попытке.
- **Что видит пользователь:** красный баннер + [Повторить]; клик по [Повторить] ничего видимо не меняет; негейченные контролы кликаются, но честно падают в toast.
- **Дельта от ожидания:** каталог «[Повторить] при мёртвом orchd — что видит юзер?» → ничего (нет busy-стейта). P-06 «реально кликабельны при down?» → да (submit CreateProject, attach-select), но с честной деградацией, не молча.
- **Действие:** BL-кандидат (Minor): busy/«переподключение…»-стейт на [Повторить] (нужен временный флаг, т.к. результат только в событиях); привести CreateProjectDialog submit и sidebar-attach к orchdDown-гейту как в идея/research-диалогах.

### A-05 — Установлен несовместимый sessiond → запуск/reconnect

- **Вердикт:** ✅ OK.
- **Проверено:** `lib.rs` connect → `ClientError::IncompatibleDaemon` (fatal, без retry, `connect_with_retry` док) → `emit_incompatible` → `daemon://incompatible` → `App.tsx:182-191` `setDaemonIncompatible(true)`+`setUpgradeDialogOpen(true)` (+ pull-fallback через `daemon_status()` на случай гонки listen, `App.tsx:337-350`). `UpgradeDialog` sessiond-ветка (`:106-210`): копия **hydrated** → «Обновить фоновый сервис — N живых сессий завершатся. Их записи и scrollback сохранены…»; **not-hydrated** → без N (finding [14]: `sessions` наполняется только успешным hydrate; при boot-incompatible slot=None → hydrate невозможен → честно не называть счёт). «Обновить»→`handleUpgradeClick` (`:75-80`): `upgradeDaemon().catch(...)` fire-and-forget (успех = never-resolve, т.к. kickstart_force+`app.restart()` убивает webview); реджект → `upgradeError` inline-строка (red, «Не удалось перезапустить… Проверьте разрешения (launchctl)…») + retry (тест :167). Копия соответствует локед-тексту.
- **Обработка ошибок:** есть, честная. `Отмена` (`:177`) `setUpgradeDialogOpen(false)` — не трогает `daemonIncompatible` (тест :136) → см. A-10 (для sessiond возврат есть).
- **Логи:** `daemon speaks an incompatible protocol version` (error, min/max).
- **Что видит пользователь:** модалка «Требуется обновление» с честной копией; «Обновить»→рестарт; при отказе upgrade — inline-ошибка + повторный «Обновить».
- **Действие:** ничего.

### A-06 — Несовместимый orchd → запуск

- **Вердикт:** ✅ OK. (Отмена → A-10.)
- **Проверено:** `bring_up_orchd` → `IncompatibleOrchd` → `emit_orchd_incompatible` → `orchd://incompatible` → `App.tsx:283-289` `setOrchdIncompatible(true)`+`setOrchdUpgradeDialogOpen(true)`. `UpgradeDialog` orchd-ветка (`:216-309`): локед-копия «Обновить фоновый сервис оркестратора — записи (проекты, цели, задачи) сохранены» — **без предупреждения о живых сессиях** (у orchd нет PTY). «Обновить»→`handleOrchdUpgradeClick` (`:84-89`): `orchdUpgrade().catch(...)` → локальный `orchdUpgradeError` (не store-поле; тест :286) → рестарт по успеху. Тесты `UpgradeDialog.test.tsx:232` (orchd-копия+`orchdUpgrade`), `:286` (rejected orchd upgrade → inline).
- **Обработка ошибок:** зеркалит sessiond (`orchd_upgrade_core` = `upgrade_daemon_core` вербатим, `commands.rs:1946-1959`): best-effort drain + honest `kickstart_force`-fail → `UpgradeFailed`.
- **Логи:** `orchd speaks an incompatible protocol version` (error).
- **Что видит пользователь:** orchd-вариант диалога; «Обновить»→рестарт; отказ→orchd-специфичная inline-ошибка.
- **Действие:** ничего (по happy). Возврат после Отмены → A-10 (там дефект).

### A-07 — Оба демона несовместимы → запуск

- **Вердикт:** ✅ OK.
- **Проверено:** precedence в `UpgradeDialog.tsx:61-65`: `sessiondOpen = daemonIncompatible && upgradeDialogOpen` проверяется ПЕРВЫМ; `orchdOpen = !sessiondOpen && orchdIncompatible && orchdUpgradeDialogOpen`; `open = sessiondOpen || orchdOpen` → рендерится ровно ОДИН диалог, sessiond безусловно выигрывает (spec §11 «sequential, sessiond first — no combined flow»). Порядок разрешения: (штатно) sessiond «Обновить»→`app.restart()`→свежий запуск→orchd re-detect→его диалог; (либо) sessiond «Отмена» → `sessiondOpen`=false → orchd-диалог показывается следующим. Оба покрыты: тест `:251` «both incompatible → SESSIOND copy (precedence), never orchd», тест `:303` «once the sessiond dialog is dismissed (Cancel), a still-pending orchd incompatibility shows its own dialog next».
- **Обработка ошибок:** н/д (чистая маршрутизация).
- **Логи:** оба incompatible-error лога (sessiond+orchd) эмитятся независимо на буте.
- **Что видит пользователь:** сначала sessiond-диалог; orchd-диалог — только после того, как sessiond перестал показываться. Никогда не оба одновременно, не комбинированный.
- **Действие:** ничего.

### A-08 — Повреждённый orchd.db → запуск

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (честность к юзеру — нарушена.)
- **Проверено:** `boot.rs:90-110 open_db_degrading` → `Db::open` (`persistence.rs:123-145`): `open_inner` форсит чтение `PRAGMA user_version` → `classify` → при `DatabaseCorrupt|NotADatabase` → `PersistError::Corrupt`. `Db::open` ловит `Corrupt` → `quarantine(path)` = `orchd.db.corrupt-<unix_ts>` (`:111-116`) → `std::fs::rename(path, dst)` → удаляет sidecar `-wal`/`-shm` → `open_inner(path)` заново (свежая БД) → **возвращает `Ok(db)`**. Демон стартует нормально, дальше персистит на диск. `warn!(?path, ?dst, "database corrupt, quarantining and recreating: …")`.
- **Ключевая проверка «говорит ли UI что-нибудь»:** НЕТ. Карантин целиком внутри `Db::open`, возвращает `Ok` → ни `open_db_degrading`, ни `boot::run`, ни `socket_server` не эмитят никакого события/флага. Греп по `src/`/протоколу на `degraded|corrupt|quarantine` — ноль каналов в UI. Список orchd-событий в `App.tsx` — только domain-changed + down/up/incompatible. **Никакого «данные были повреждены и отправлены в карантин»-сигнала нет.**
- **Обработка ошибок:** на уровне демона честная и корректная (не паника, данные не уничтожены — карантинный файл восстановим оператором). Дефект — на уровне **прозрачности для юзера**.
- **Логи:** `warn!` с `path`/`dst` (без секретов) — только в orchd-логе, недоступном юзеру в UI.
- **Что видит пользователь:** пустой аккаунт (0 проектов/целей/задач/идей) без единого объяснения — неотличимо от «свежая установка». Он не знает, что (а) данные были, (б) они в `orchd.db.corrupt-<ts>`, (в) их можно попытаться восстановить.
- **Дельта от ожидания:** каталог «Данные исчезли — говорит ли UI об этом хоть что-нибудь? Честность: юзер видит пустой аккаунт без объяснения?» → **UI молчит; юзер видит пустой аккаунт без объяснения — честность нарушена.**
- **Действие:** BL-кандидат (Important): эмитить одноразовое событие/баннер при карантине БД («обнаружено повреждение базы; данные сохранены в резервной копии orchd.db.corrupt-<ts>, продолжаем с чистой базой») — protocol/Hello может нести флаг `db_recovered`, UI показывает dismissible-баннер.

### A-09 — Диск/директория недоступна → запуск orchd

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (граничит с 🔴 — silent data-loss-on-restart.)
- **Проверено:** `boot.rs:90-110 open_db_degrading`: `Db::open` возвращает `Err` для НЕ-corruption-ошибок — `open_inner` при недоступной директории/файле → `PersistError::Open("create dir failed…"/…)` (не Corrupt) → `Db::open` матчит `Err(other) => Err(other)`. Тогда `open_db_degrading` (95-108): `error!(error=%e, path, "DB open failed; continuing in degraded (in-memory) mode")` → `Db::open_in_memory()` → in-memory БД. Демон живёт весь lifetime на in-memory (`boot::run:192`). Только отказ самого in-memory-фолбэка = паника (недостижимо в норме).
- **Ключевая проверка «есть ли индикация неперсистентного режима»:** НЕТ. In-memory-режим не отражается ни в каком событии/ответе/Hello. UI неотличим от нормального. **Всё, что юзер создаёт (проекты, идеи, задачи, инсайты, граф) — живёт только в RAM демона и исчезает при его рестарте/краше без единого предупреждения.**
- **Обработка ошибок:** демон деградирует «честно» в своём логе, но UI-контракт про режим не знает → для юзера это тихий data-loss-режим.
- **Логи:** `DB open failed; continuing in degraded (in-memory) mode` (error, path) — только в orchd-логе.
- **Что видит пользователь:** полностью рабочее приложение; создаёт данные весь сеанс; при следующем запуске (или краше orchd, KeepAlive перезапустит) — всё пусто, без объяснения. Опаснее A-08: там данные хотя бы в карантине, здесь их не было на диске вообще.
- **Дельта от ожидания:** каталог «Юзер не знает, что данные НЕ сохраняются; есть ли любая индикация неперсистентного режима» → **индикации нет.** Это самый серьёзный из A-08/A-09.
- **Действие:** BL-кандидат (Important, кандидат в 🔴): постоянный persistent-баннер «работаем в непостоянном режиме — данные не сохраняются на диск» при in-memory-фолбэке (флаг в Hello/статусе → UI-баннер, не dismissible).

### A-10 — UpgradeDialog открыт → «Отмена»

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (для orchd-варианта; sessiond — OK.)
- **Проверено:**
  - «Отмена» sessiond (`UpgradeDialog.tsx:177`) → `setUpgradeDialogOpen(false)`; orchd (`:276`) → `setOrchdUpgradeDialogOpen(false)`. Оба **НЕ трогают** `daemonIncompatible`/`orchdIncompatible` (фатальный флаг остаётся; тесты `:136`, `:278`). Мутации остаются мертвы (slot=None → `Disconnected`).
  - **Возврат к диалогу — sessiond:** DaemonBanner incompatible-ветка (`DaemonBanner.tsx:25-58`) при `daemonIncompatible` показывает «Фоновый сервис устарел — требуется обновление» + кнопку «Обновить» → `setUpgradeDialogOpen(true)` → **диалог возвращается** (тест `DaemonBanner.test.tsx:52`). ✅
  - **Возврат к диалогу — orchd:** после Cancel `orchdIncompatible=true`, но **`orchdDown=false`** (incompatible-путь `bring_up_orchd:583-591` зовёт ТОЛЬКО `emit_orchd_incompatible`, не `emit_orchd_down`; `store.ts:218` комментарий: orchdDown флипается только `orchd://down`/`up`). Греп readers `orchdIncompatible`: только `UpgradeDialog` (сам диалог) + `QuickCapture` (блок ⌘K) + `App.tsx` (коммент). **Ни один БАННЕР не читает `orchdIncompatible`.** `OrchdDownBanner` монтируется только при `orchdDown===true` → не показывается. ⇒ **после Cancel — пустой экран, ноль индикации что orchd несовместим, ноль пути вернуть диалог до рестарта app.**
- **Обработка ошибок:** флаг-инвариант честен (фатальность переживает Cancel). Дыра — асимметрия возвратного пути между демонами.
- **Логи:** н/д на Cancel.
- **Что видит пользователь:** sessiond — баннер «устарел» с «Обновить» (возврат есть). orchd — после Cancel НИЧЕГО (нет баннера); orchd-мутации не дизейблены (orchdDown=false) но падают в toast «оркестратор недоступен» при попытке; вернуть upgrade-диалог нельзя.
- **Дельта от ожидания:** каталог «Как вернуть диалог? Возвратный путь существует?» → для sessiond да, **для orchd — нет**. «Все мутации мертвы?» → да (slot None), но без баннера, объясняющего почему.
- **Действие:** BL-кандидат (Important): баннер, читающий `orchdIncompatible` (зеркало DaemonBanner incompatible-ветки) с кнопкой «Обновить» → `setOrchdUpgradeDialogOpen(true)`; либо на incompatible-пути также выставлять индикатор, чтобы orchd-несовместимость была видима и обратима после Cancel.

---

## Сводка ключевого

1. **A-08 / A-09 — честность к юзеру про состояние данных.** Оба пути деградации БД (карантин повреждённой / in-memory при недоступном диске) корректны на уровне демона и логируются, но **UI не сообщает ничего**: A-08 — пустой аккаунт без объяснения (данные в восстановимом `orchd.db.corrupt-<ts>`); A-09 — полностью рабочее приложение, где ничего не персистится и всё теряется при рестарте, без единого предупреждения (самый опасный, граничит с 🔴). Нужен канал (флаг в Hello/статусе → баннер).
2. **A-10 (orchd) — потерянный upgrade-диалог.** После «Отмена» на orchd-варианте `orchdIncompatible` остаётся, но `orchdDown=false` и ни один баннер не читает `orchdIncompatible` → пустой экран, диалог не вернуть до рестарта. Для sessiond симметричный возврат есть (DaemonBanner «Обновить»).
3. **A-01 — под-информативная деградация bring-up.** Окно всегда открывается, паники нет, но жёсткий отказ launchctl (TCC/права/нет бинаря) показывается как generic «reconnecting…» / «Оркестратор недоступен»; типизированный `LaunchdError` только логируется. «reconnecting…» вводит в заблуждение при перманентном отказе. + F-6 (нет онбординга).
4. **A-03 / A-04 — мелкие фидбек-гэпы.** A-03: reconnect/re-attach образцовы, но ввод в мёртвый PTY молча теряется (fire-and-forget `writeStdin` без гейта/эха). A-04: [Повторить] даёт ноль фидбека; P-06-контролы (CreateProject submit, sidebar-attach) не гейтятся, но падают в честный toast.
5. **Хорошо (✅):** A-05/A-06/A-07 — upgrade-флоу честен, копии соответствуют локед-текстам, hydrated-гейт счётчика честен (finding [14]), precedence sessiond-first строго соблюдён, `.catch` на upgrade ловит единственный честный отказ.

**Не удалось проверить рантаймом:** реальный первый запуск .app с настоящим launchd/TCC-денайлом и с настоящей повреждённой/недоступной orchd.db (SAFETY: не трогаю launchd/`~/Library/Application Support` реальной машины). Вердикты A-01/A-08/A-09 построены статически на исходниках (`launchd.rs`, `lib.rs::bring_up_*`, `boot.rs::open_db_degrading`, `persistence.rs::Db::open`/`quarantine`) + существующих юнит-тестах в этих файлах (`hard_failure_surfaces_install_error`, `ensure_daemon_running_uses_non_force_kickstart_on_boot`, `is_loaded_reads_print_exit_code`, `reconcile_interrupted_research_runs_on_fresh_db_is_a_noop`, `ensure_global_ruleset_*`). Фронт-вердикты (A-02/A-05/A-06/A-07/A-10) подтверждены прогоном `npx vitest run UpgradeDialog/DaemonBanner/OrchdDownBanner/HomeView/WorkspaceSidebar` → **50 passed** (в т.ч. `HomeView.test.tsx:205` zero-workspace-no-action, `UpgradeDialog.test.tsx:251/278/303` precedence/cancel).

# Эпик H — Задачи (фичи): инвестигейт H-01..H-07

> READ-ONLY инвестигейт по каталогу `docs/qa/ux-first-session-scenarios.md` §2 Эпик H.
> Модель: opus. Пути прослежены UI-контрол → ipc → wire → dispatch → persistence.
> Тесты: `npx vitest run src/components/TasksList.test.tsx` → **13 passed**;
> `cargo test -p bpa-orchd --lib task` → **26 passed** (create/update/status/rank/delete/cascade/cycle).

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| H-01 | 🟡 UX-GAP | Minor | Happy-create + error-toast + сохранение полей + orchdDown-гейт — всё ✅; но **P-19**: «+ задача» без in-flight-гварда → двойной клик = две задачи (теста нет) |
| H-02 | 🟡 UX-GAP | Minor | Связь с инсайтом сохранена в БД (`source=insight`, `sourceId=insight.id`), но `TaskRow` рендерит только title/status/▲▼/Удалить — источник и sourceId **невидимы в UI** |
| H-03 | ✅ OK | — | any→any **осознанно**: спека §5.2 без transition-инварианта, `set_task_status` = голый `UPDATE`, селект даёт все 6 статусов (kanban-free; доска — S5) |
| H-04 | 🟡 UX-GAP | Minor | Rank-математика **collision-safe** (один вызов, midpoint distinct-f64 либо край ±1024; same-row=идемпотентно, different-row=distinct) — ни коллизий, ни jumble; но push-only refresh → в окне латентности двойной ▲ = второй клик no-op; теста на гонку нет |
| H-05 | ✅ OK | — | cycle-Invariant **недостижим из UI**: parent-select только в create-форме (новая задача без id/потомков, reparent-verb-а нет) → селект НЕ включает саму задачу/потомков; серверный guard защитный (прямой тест) |
| H-06 | ✅ OK | — | confirm называет точное число потомков (рекурсивный `countDescendants`); каскад через FK `ON DELETE CASCADE` + `foreign_keys=ON`; тесты `delete_task_cascades_subtasks` + «удалит 2 подзадач» зелёные |
| H-07 | ✅ OK | — | cross-project parent **недостижим**: селект листает только `tasksByProject[projectId]` (текущий проект); сервер всё равно защищает `Invariant` (тест `create_task_cross_project_parent_is_invariant` зелёный) |

**Итог по эпику:** 4×✅ OK · 3×🟡 UX-GAP (все Minor). Ноль 🔴, ноль 📄.

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| P-19 (двойной сабмит) | 🟡 подтверждён для `TasksList` «+ задача» | H-01 |
| F-5 / BL-61 / O-2 (mixed ru/en в Invariant-тексте) | 🟡 подтверждён, но недостижим | H-05/H-07 |

---

## H-01 — Таб «Задачи»: заполнить форму → «+ задача»

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** Happy-path и обработка ошибок ✅; double-submit (P-19) подтверждён.
- **Проверено:** `TasksList.tsx:384-403 handleCreate` → `orchdCreateTask(projectId, parentId, title, createBody, null, createSource, null, tags)` (ipc/orchd.ts:235-255) → `OrchdRequest::CreateTask` (socket_server.rs:1106-1130) → `db.create_task` (persistence.rs:2149-2220) → `respond_task` пушит `TasksChanged{project_id}` (socket_server.rs:565-574). Тест «the create dialog passes source, parent, and comma-split tags correctly» + «create submit disabled while title blank» — зелёные.
- **Обработка ошибок:** есть, честная. Submit `disabled={orchdDown || createTitle.trim()===""}` (463); теги режутся `split(",").map(trim).filter(!=="")` (387-390); `parentId = createParentId===""?null:createParentId` (391). На успехе — очистка всех пяти полей (395-398) + `refreshTasks(projectId)` (399). На отказе — `showToast(describeOrchdError(e))` (401), поля **НЕ** сбрасываются (сброс только в success-ветке до `await`-резолва) → «поля сохранены» соблюдено.
- **Логи:** UI-слой — toast. Демон `create_task` — INSERT без пер-верб tracing (класс B-04, системное решение). Секретов нет.
- **Что видит пользователь:** новый ряд в группе `бэклог` (дефолт-статус когда `status=null`, persistence.rs:2195), форма сброшена, счётчик группы +1. Rank новой задачи = `MAX(rank)+1024` scoped-по-проекту (persistence.rs:2186-2191) → всегда в конце.
- **Дельта от ожидания (P-19):** **на «+ задача» НЕТ in-flight/busy-гварда.** `disabled` завязан только на `orchdDown||blank-title`; `createTitle` (React state) очищается лишь ПОСЛЕ резолва `await orchdCreateTask` (строка 395). В окне между кликом и резолвом `createTitle` остаётся непустым → кнопка активна → быстрый второй клик снова зовёт `handleCreate` с тем же title → **два `orchdCreateTask` → две задачи.** Контраст: `ConnectDialog` держит `disabled={busy}`. Теста на double-submit для `TasksList` нет. Цена ниже, чем у F-08 (нет внешнего вызова/spend — просто дубль-ряд, легко удаляемый) → Minor.
- **Действие:** BL/фикс (Minor): добавить `busy`-гвард на «+ задача» (зеркально ConnectDialog) — единый паттерн с P-19 по всем диалогам.

## H-02 — Из G-03: найти задачу-из-инсайта (видна ли связь с инсайтом в UI?)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** данные связи КОРРЕКТНЫ. `FormInsightDialog.tsx:289-313 handleBacklog` зовёт `orchdCreateTask(idea.projectId, null, insight.title, insight.body, null, "insight", insight.id, [])` — т.е. `source="insight"` И `source_id=insight.id` реально записываются (persistence.rs:2197-2215 INSERT ... source, source_id). Верб/схема хранят `source_id TEXT` (persistence.rs:309). Рендер: `TaskRow` (`TasksList.tsx:226-281`) выводит ТОЛЬКО `task.title` (span 233-235), status-select, ▲/▼, «Удалить». **Ни `task.source`, ни `task.sourceId`, ни `tags`/`body` не рендерятся нигде в списке задач.** `SOURCE_LABEL` (TasksList.tsx:31-36) используется исключительно в create-форме (селект источника, стр. 434) — не в ряду. Единственный `.source`-рендер в кодовой базе — `InsightsList.tsx:221 insight.source`, к задачам отношения не имеет.
- **Обработка ошибок:** н/д (визуальный вопрос, не ошибка).
- **Логи:** н/д.
- **Что видит пользователь:** обычный ряд задачи с заголовком инсайта — **неотличимый** от задачи, созданной вручную. Нет бейджа «из инсайта», нет ссылки/крышки на исходный insight, нет отображения `sourceId`. Провенанс инсайт→задача полностью теряется в UI (перекликается с F-7/BL-84 «provenance-крошек нет»).
- **Дельта от ожидания:** каталог H-02 «source=insight, sourceId корректен» — на уровне ДАННЫХ да (доказано вызовом в FormInsightDialog). «Видна ли связь с инсайтом в UI» — **нет**, невидима полностью.
- **Действие:** BL-кандидат (Minor): в `TaskRow` показывать бейдж источника (`SOURCE_LABEL[task.source]`) и, для `source∈{insight,idea}`, кликабельную крошку к исходной сущности по `sourceId`. Behavior-safe, чисто observability.

## H-03 — Задача: гонять статусы (any→any, state-machine нет)

- **Вердикт:** ✅ OK (by-design).
- **Проверено:** UI — `TaskRow` status-`<select value={task.status}>` (TasksList.tsx:236-249) безусловно рендерит все шесть `STATUS_VALUES` как опции → любой статус выбираем из любого. Верб — `set_task_status` (persistence.rs:2267-2291): голый `UPDATE task SET status=?2` без проверки перехода. Спека — S3 `§5.2 Invariants` (design-doc:370-382): для задач перечислены ТОЛЬКО «parent same-project + cycle» и rank-математика; **строки про transition-graph/state-machine НЕТ**. `SetTaskStatus{id,status}` (spec:218) — параметр-статус без ограничений. Тест `set_task_status_updates_status_and_db_literal` — зелёный.
- **Обработка ошибок:** отказ (напр. archived-project → `Invariant`) → `handleStatusChange` (329-335) `showToast(describeOrchdError)`. Тест «a rejecting status-change mutation surfaces via showToast» — зелёный.
- **Логи:** пер-верб нет (B-04); toast на UI.
- **Что видит пользователь:** `бэклог → готово` (или любой другой) одним кликом проходит. Осознанно: доска-kanban с free-movement — S5 (design-doc:27 «kanban board (S5)»); в v1 статус — плоский enum-select без машины состояний. Минорная косметика: `<select>` контролируемый по `value={task.status}` без оптимистика — до прихода push`а нативный селект кратко «отскакивает» к старому значению (локальный roundtrip — миллисекунды).
- **Дельта от ожидания:** нет. Каталог «Осознанно ли (backlog→done одним кликом)» — да, спека сознательно не задаёт state-machine.
- **Действие:** ничего. (O-опционально: если бизнес захочет ограничить переходы — это новый инвариант, сейчас его нет намеренно.)

## H-04 — Группа задач: ▲/▼ (rank, push-only; гонка двух реордеров)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** Данные collision-safe; гэп — только push-only stale-окно + отсутствие теста на гонку.
- **Проверено (rank-математика):** `handleMoveUp` (TasksList.tsx:352-359) / `handleMoveDown` (364-371) считают `newRank` из ТЕКУЩЕГО `group` (= `groupByStatus(tasksByProject[projectId])`, отсортирован по rank asc): между двумя соседями — их midpoint `(prevPrev.rank+prev.rank)/2` / `(next.rank+nextNext.rank)/2`; на краю (нет дальнего соседа) — `firstRank-1024` / `lastRank+1024` (`RANK_GAP=1024`). ОДИН вызов `orchdSetTaskRank(id, newRank)` (337-343) → `set_task_rank` пишет f64 verbatim (persistence.rs:2296-2316, «fractional insert-between is the CLIENT's move»). Тесты midpoint/edge (5 UI-кейсов) + `set_task_rank_persists_f64_midpoint` — зелёные.
- **Гонка двух быстрых реордеров (проанализировано):** refresh **только по push** — `applyRank` НЕ зовёт `refreshTasks`; статус/rank-правки полагаются на `orchd://tasks-changed → refreshTasks` (App.tsx:202, док-коммент TasksList.tsx:290-295). Значит в окне латентности push`а второй клик считает newRank по СТАРЫМ ранкам. Разбор:
  1. **Тот же ряд дважды** (двойной ▲ на C при A,B,C,D): оба клика по одинаковому stale-`group` считают ОДИН и тот же midpoint `(A+B)/2` → второй `orchdSetTaskRank` пишет то же значение → **идемпотентно** (ряд едет на 1 позицию, не на 2). Не коррупция — но «▲ будто не сработал второй раз».
  2. **Разные ряды**: каждый использует РАЗНУЮ пару соседей → разные midpoint-значения; для distinct f64 midpoint distinct от обоих концов → **коллизии рангов нет**.
  3. Край (floor/ceiling): `firstRank-1024` достижим лишь для ряда на idx=1 с одним и тем же первым рядом → повтор = то же значение (идемпотентно), два РАЗНЫХ ряда одновременно floor не берут.
  ⇒ **Ни коллизий рангов, ни «jumble».** Даже гипотетический tie ломается сервером `ORDER BY rank, id` (persistence.rs:2347) и клиентским stable-sort — не краш, максимум неоднозначный порядок.
- **Обработка ошибок:** отказ `applyRank` → `showToast(describeOrchdError)` (341); края — кнопки `disabled` (`canMoveUp=idx>0`, `canMoveDown=idx<len-1`, стр. 254/264/490-491). Тест «▲ on first / ▼ on last disabled» — зелёный.
- **Логи:** UI-toast; демон — UPDATE без tracing (B-04).
- **Что видит пользователь:** порядок меняется после прихода push`а (`TasksChanged → refreshTasks` перечитывает весь список с серверными ранками). В окне латентности список показывает СТАРЫЙ порядок (нет оптимистика) → двойной ▲ визуально продвигает на 1 позицию (второй клик — тихий no-op, п.1 выше).
- **Дельта от ожидания:** каталог «гонка двух реордеров; ранги collide/jumble? Любой тест?» — **коллизий/jumble НЕТ** (математика безопасна); теста на быструю гонку нет; единственный реальный минус — stale-окно push-only + идемпотентный второй клик.
- **Действие:** BL-кандидат (Minor): оптимистичное локальное применение rank (или лёгкий disable на время in-flight) убрало бы stale-окно и «no-op второго клика»; плюс тест на rapid-double-reorder. Данным ничего не угрожает — приоритет низкий.

## H-05 — Форма задачи: parent = сама задача / потомок

- **Вердикт:** ✅ OK (защитный guard; путь недостижим из v1-UI).
- **Проверено:** parent-`<select>` существует ТОЛЬКО в create-форме (`TasksList.tsx:438-451`), его опции = `tasks.map(...)` где `tasks = tasksByProject[projectId] ?? []` (стр. 311) — существующие задачи текущего проекта. Т.к. форма СОЗДАЁТ новую задачу (id ещё нет, потомков нет), **селект структурно не может включать саму задачу или её потомков.** Reparent/edit-parent верба в v1 НЕТ: `orchdUpdateTask` принимает только `title/body/tags` (ipc/orchd.ts:257-264; persistence.rs:2225-2263). Серверный cycle-guard `task_ancestor_chain_contains` (persistence.rs:1243-1263) вызывается в `create_task` защитно против собственного pre-generated id (persistence.rs:2179-2183), но, как гласит его док (1235-1242), «в v1 нет reparent-verb-а → новая задача не может быть своим предком, эта ветка не триггерится через публичный API». Функция покрыта ПРЯМЫМ тестом `task_ancestor_chain_contains_detects_direct_and_transitive_cycle` (не через create_task) — зелёный.
- **Обработка ошибок:** серверный guard честный и типизированный (`Invariant`), но недостижим. Реальная достижимая ошибка из parent-селекта — гонка «родителя удалили в другом окне между populate и submit» → `parent.ok_or(NotFound)` (persistence.rs:2173) → тост «не найдено».
- **Логи:** пер-верб нет (B-04).
- **Что видит пользователь:** в норме — ничего проблемного; сам себя/потомка выбрать нельзя. ЕСЛИ бы cycle-Invariant всплыл (будущий reparent), `describeOrchdError` (ipc/orchd.ts:769-770) даст `недопустимая операция: cannot create a task under itself or one of its own descendants` — **ru-префикс + en-тело** (смешанный язык, откр. вопрос O-2 / класс F-5/BL-61).
- **Дельта от ожидания:** каталог «does the parent select really include the task itself/descendants?» — **нет**, невозможно (create-форма). «Текст ошибки читаем?» — читаем, но mixed ru/en (moot, т.к. недостижим).
- **Действие:** ничего для v1. Заметка на будущее: когда появится reparent-verb — нужен клиентский cycle-guard в селекте + локализация Invariant-текста (O-2).

## H-06 — Задача с подзадачами: «Удалить» (confirm с числом потомков → каскад)

- **Вердикт:** ✅ OK.
- **Проверено:** `handleDelete` (TasksList.tsx:373-382): `countDescendants(tasks, task.id)` (60-73 — рекурсивный обход по `parentId`, не только прямые дети) → `deleteConfirmText(n)` (52-55: `n===0 → "удалить задачу?"`, иначе `"удалить задачу? удалит N подзадач"`) → `window.confirm` gate → `orchdDeleteTask` → `refreshTasks`. Каскад: FK `parent_id TEXT REFERENCES task(id) ON DELETE CASCADE` (persistence.rs:304) + `PRAGMA foreign_keys=ON` установлен на ОБОИХ путях — on-disk (persistence.rs:154) и in-memory (:202). `delete_task` = простой `DELETE WHERE id=?1` (persistence.rs:2321-2336), поддерево уносит FK. Тесты: `delete_task_cascades_subtasks` (backend) + «delete … shows «удалит 2 подзадач» warning and only calls orchdDeleteTask after confirm» + «no children confirms without naming a subtask count» — зелёные.
- **Обработка ошибок:** confirm=false → ранний `return`, вызова нет (тест доказывает). Отказ `orchdDeleteTask` → `showToast(describeOrchdError)` (380-381). Структурная мутация → явный `refreshTasks(projectId)` (378) поверх push.
- **Логи:** UI-toast; демон — DELETE без tracing (B-04).
- **Что видит пользователь:** для задачи с детьми — честный confirm «удалит N подзадач» (точный рекурсивный счёт), после подтверждения — ряд и всё поддерево исчезают.
- **Дельта от ожидания:** нет.
- **Действие:** ничего.

## H-07 — Верб: cross-project parent (достижимо ли из UI?)

- **Вердикт:** ✅ OK (недостижимо из UI; сервер всё равно защищает).
- **Проверено:** parent-`<select>` листает `tasks = tasksByProject[projectId]` (TasksList.tsx:311, 446) — задачи ТОЛЬКО текущего проекта; чужого проекта в опциях нет → cross-project parent из UI не выбрать. Селект де-факто фильтрует по проекту (по построению слайса). Сервер: `create_task` сверяет `parent_project != project_id` → `Invariant("task parent_id must belong to the same project")` (persistence.rs:2174-2178). Тест `create_task_cross_project_parent_is_invariant` — зелёный.
- **Обработка ошибок:** серверный guard честный/типизированный, но из UI не достигается.
- **Логи:** пер-верб нет (B-04).
- **Что видит пользователь:** ничего проблемного — выбрать чужой проект нельзя. (ЕСЛИ бы всплыло — `недопустимая операция: task parent_id must belong to the same project`, mixed ru/en, как H-05.)
- **Дельта от ожидания:** каталог «Достижимо ли из UI (select фильтрует по проекту?)» — **нет, недостижимо**; селект скоуплен на текущий проект.
- **Действие:** ничего.

---

## Сводка ключевого

1. **H-02 — 🟡 Minor, самая содержательная находка эпика.** Связь инсайт→задача сохраняется в БД честно (`source=insight`, `sourceId=insight.id` — доказано вызовом в `FormInsightDialog.handleBacklog`), но `TaskRow` рендерит только заголовок+контролы: источник и sourceId **не видны нигде** в списке задач. Провенанс полностью теряется в UI (смежно F-7/BL-84).
2. **H-01 / P-19 — 🟡 Minor.** «+ задача» без in-flight-гварда (`disabled` только `orchdDown||blank-title`, `createTitle` очищается лишь после резолва) → двойной клик = две задачи. Цена ниже F-08 (нет внешнего spend). Теста нет.
3. **H-04 — 🟡 Minor, но данные безопасны.** Rank-математика collision-safe: один вызов, midpoint distinct-f64 либо край ±1024 — same-row-повтор идемпотентен, different-row дают distinct-значения. **Ни коллизий, ни jumble.** Единственный минус — push-only refresh даёт stale-окно, в котором двойной ▲ = второй клик no-op (ряд едет на 1 позицию). Теста на гонку нет.
4. **H-03 — ✅ by-design.** any→any статус осознан: спека §5.2 не задаёт transition-машину; доска-kanban отложена в S5.
5. **H-05 / H-07 — ✅ недостижимо.** cycle- и cross-project-Invariant`ы — защитные серверные guard`ы, недостижимые из v1-UI: parent-select живёт только в create-форме и скоуплен на текущий проект; reparent-verb-а нет. Если бы всплыли — текст mixed ru/en (O-2/F-5/BL-61).
6. **H-06 — ✅ полностью.** confirm с точным рекурсивным счётом потомков; каскад через FK + `foreign_keys=ON` (оба пути); всё покрыто зелёными тестами.

**Не удалось проверить рантаймом:** реальную гонку двух физических кликов ▲/▼ в живом webview (нет запущенного стенда Tauri) — вердикт H-04 построен статически на коде `handleMoveUp/Down` + `applyRank` (push-only, без оптимистика) + анализе midpoint-математики для stale-`group`; коллизия/jumble опровергнуты рассуждением (идемпотентность same-row, distinct-соседи different-row), не рантайм-стресс-тестом. Двойной сабмит «+ задача» (H-01/P-19) также подтверждён статически (нет busy-гварда; `createTitle` очищается после `await`), не физическим дабл-кликом.

## Волна 3 — эпики J, B, I (2026-07-16)

# Эпик J — Расширения (прочее): инвестигейт J-01..J-08

> READ-ONLY инвестигейт по каталогу `docs/qa/ux-first-session-scenarios.md` §2 Эпик J.
> Модель: opus. Пути прослежены UI-контрол → ipc → command → wire → dispatch → модуль.
> Тесты прогнаны: `cargo test -p bpa-orchd --lib begin_oauth` → **2 passed**;
> `npx vitest run src/components/ext/{ToolsBrowser,ConnectorsTab,SkillsTab,InvocationLog}.test.tsx`
> → **52 passed**.

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| J-01 | 🟡 UX-GAP | Minor | Отказ тоггла тула → только toast, нет on-row сигнала; НО чекбокс контролируемый (`checked={tool.enabled}`, без оптимистичного флипа) → на отказе остаётся на серверном значении, НЕ застревает/не стухает — бага нет |
| J-02 | 🟡 UX-GAP | Minor | Policy/Consent-отказ вызова тула честен (inline `role=alert` + toast, `ToolDisabled→Policy`, `ConsentRequired→Consent`), НО P-20: ноль пути к ConnectDialog (он смонтирован РОВНО в ServersTab) — восстановление недискаверабельно |
| J-03 | ✅ OK | — | Add API-ключ: маскирование честное (`type=password`, поле очищается на успехе, ряд ключ не рендерит), Keychain-fail → `Io` toast, ключ сохранён для ретрая; orchdDown гейтит submit |
| **J-04** | **🟡 UX-GAP** | **Minor** (спорно Important) | **Реестр провайдеров ПУСТ на буте** (`boot.rs:205` `ConnectorsState::new()`, ноль `register_oauth_provider` вне тестов) → «начать OAuth» с любым провайдером = toast **«ошибка сервиса: unknown OAuth provider: <X>»**; кнопка полностью активна, БЕЗ «скоро» (в отличие от ServersTab/SkillsTab) — гарантированный тупик с вводящей в заблуждение копией |
| J-05 (+B-02) | 🟡 UX-GAP | Minor сценарий / **B-02 = latent Critical** | `ssrf_guarded_http_client` (accounts.rs:491-496) БЕЗ `.timeout()` (контраст adapter.rs:111 `.timeout(30s)`) — `complete_oauth`/`refresh` могут висеть вечно и клинить весь orchd (как F-03/B-01); НО **в v1 недостижимо** (пустой реестр → нет challenge → «завершить» не рендерится); P-06: `oauth-code-input` не гейтится orchdDown (косметика) |
| J-06 | 🟡 UX-GAP | Minor | P-15: отказ `connectorListOps` → `opsByAccount[id]` не заполняется → селект пуст навсегда, нет retry, неотличимо от «нет операций»; смягчение: для единственного адаптера (generic-rest) `list_ops` статичен и почти неотказуем |
| J-07 | ✅ OK + 🟡/📄 | Minor | Add/delete навыка честны (валидация типизирована и показана, orchdDown-гейт); P-16: ошибка нативного пикера (`CommandError::Internal`) через `describeOrchdError` → generic «неизвестная ошибка оркестратора» (message теряется, blast-radius ничтожный); **F-1: frontend-conventions.md ПРАВ (точный матч), architecture.md:530 ВРЁТ (парафраз)** |
| J-08 | 🟡 UX-GAP | Minor | Не-число → toast «предел должен быть числом», без inline; BL-78 подтверждён: delete/reset-политики нет НИ в UI, НИ как верб (grep: `NONE FOUND`); P-23 подтверждён: пуша `audit-changed` НЕ существует во всём OrchdPush-enum → «Аудит» живёт только ремоунтом таба |

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где |
|---|---|---|
| **B-02** (OAuth token-exchange без timeout) | **🔴 Critical — ПОДТВЕРЖДЁН статически, но DORMANT в v1** | J-05 |
| P-15 (listOps-fail → пустой селект навсегда) | 🟡 подтверждён | J-06 |
| P-16 (picker-error через describeOrchdError → generic) | 🟡 подтверждён (негативно) | J-07 |
| P-20 (Policy/Consent-отказ без пути к ConnectDialog) | 🟡 подтверждён | J-02 |
| P-23 (аудит без live-пуша) | 🟡 подтверждён (пуша нет в proto вообще) | J-08 |
| P-06 (OAuth code-input не гейтится orchdDown) | 🟡 подтверждён (косметика) | J-05 |
| P-19 (двойной сабмит — connector-мутации без busy-гварда) | 🟡 подтверждён (кросс-каттинг) | J-03/J-04/J-05 |
| BL-78 (delete/reset политики нет) | ✅ подтверждён (нет верба) | J-08 |
| F-1 (копия баннера «Навыки») | 📄 DOC-GAP — architecture.md:530 неверна | J-07 |

---

## J-01 — «Инструменты»: чекбокс «включён» (тоггл через push)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `ToolsBrowser.tsx:145-152 handleToggle` → `mcpSetToolEnabled(tool.id,!tool.enabled)` → `refreshMcpTools(tool.serverId)`. Wire: `socket_server.rs:1574-1588` — на успехе push `McpToolsChanged{server_id}`, на отказе `map_err(e)` (без пуша). Чекбокс: `ToolsBrowser.tsx:204-211` `checked={tool.enabled}`, `disabled={orchdDown}`, `onChange={handleToggle}`. Тест `ToolsBrowser.test.tsx:92` (toggle зовёт `mcpSetToolEnabled` с флипнутым флагом) — зелёный.
- **Обработка ошибок:** есть, честная. `handleToggle` catch → `showToast(describeOrchdError(e))`. **Оптимистичного локального флипа НЕТ** — компонент не держит собственный `enabled`-стейт; чекбокс полностью контролируется `tool.enabled` из стора. На отказе `await mcpSetToolEnabled` бросает ДО `refreshMcpTools` → стор не мутируется → ре-рендер из `checked={tool.enabled}` возвращает чекбокс к серверному значению. То есть чекбокс НЕ застревает в неверном визуальном состоянии и НЕ стухает — бага нет.
- **Логи:** UI-слой — toast. Демон `set_mcp_tool_enabled` — обычный update (B-04-класс, вне эпика).
- **Что видит пользователь:** при отказе — чекбокс мгновенно возвращается к прежнему значению + 4-секундный toast с описанием ошибки. **НЕТ per-row inline-сигнала отказа** — в отличие от «вызвать» (`tool-call-error`, `role=alert`, стойкий) в ТОМ ЖЕ файле. Toast — единственный след (один слот, автозакрытие 4с, P-21).
- **Дельта от ожидания:** каталог «чекбокс визуально не меняется?» → да, не меняется (остаётся на серверном значении). «Ряд без сигнала отказа» → подтверждено: только toast, ноль on-row.
- **Действие:** ничего критичного (поведение корректно). Опциональный UI (Minor): per-row inline-ошибка тоггла зеркально `tool-call-error`.

## J-02 — Тул enabled: args JSON → «вызвать» (Policy/Consent-отказ)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-20 подтверждён.)
- **Проверено:** `ToolsBrowser.tsx:154-175 handleCall`: `JSON.parse` в try/catch → inline `tool-call-error` (bad JSON, вызова нет); успех/отказ `mcpCallTool` → на отказе `setCallError(...)` (inline, стойкий) + `showToast(...)`. Wire: `socket_server.rs:1596-1626 McpCallTool` → `map_mcp_err` (`socket_server.rs:401`): `ToolDisabled|PolicyCapExceeded → Error{Policy}`, `ConsentRequired → Error{Consent}`. `describeOrchdError` (orchd.ts:781/779): Policy → «запрещено политикой: …», Consent → «требуется согласие на подключение: …».
- **Обработка ошибок:** есть, честная — сообщение показано И inline (`role="alert"`, переживает clobber toast-очереди) И toast. Это лучше J-01.
- **Что видит пользователь:** красная строка + toast с человекочитаемым (для Policy/Consent) префиксом. **НО** (P-20): ноль навигационного пути к ConnectDialog. Грепом подтверждено: `ConnectDialog` смонтирован РОВНО в одном месте — `ServersTab.tsx:367`. При Consent-отказе (устаревший grant / смена URL) юзер должен сам догадаться пойти в таб «Серверы» → «подключить» → заново consent. Путь восстановления существует, но недискаверабелен из точки отказа.
- **Дельта от ожидания:** каталог «нет пути к ConnectDialog из отказа» → подтверждено; качество recovery-пути низкое.
- **Действие:** UI (Minor): при коде Consent показывать в inline-ошибке ссылку/кнопку «перейти к подключению сервера». Не блокер.

## J-03 — «Коннекторы»: «+ API-ключ» (маскирование + Keychain-fail)

- **Вердикт:** ✅ OK.
- **Проверено:** `ConnectorsTab.tsx:274-290 handleAddApiKey`: `apiKeyBlocked` (провайдер/метка/ключ пустые) → submit disabled (531). На успехе — `setApiKeyProvider("")`/`setApiKeyLabel("")`/`setApiKeyValue("")` (282-285) + `refreshAccounts`. Поле ключа `type="password"` (520). Wire `socket_server.rs:1728-1746` → `add_apikey` (accounts.rs:334-357: Keychain `set` ПЕРЕД insert). Тест `ConnectorsTab.test.tsx:107` («ключ маскируется и очищается после submit») — зелёный.
- **Обработка ошибок:** есть, честная. Keychain-fail → `ConnectorError::Secret` → `map_connector_err` (socket_server.rs:443, arm `other`) → `Error{Io}` → describeOrchdError → «ошибка сервиса: …» toast. На ОТКАЗЕ поле ключа НЕ очищается (очистка только в success-ветке) → ключ сохранён в маскированном инпуте для ретрая.
- **Логи:** секрет не логируется (`bpa_secrets` контракт; `AccountToken`/`OAuthProviderConfig` Debug редактируют — тесты `*_debug_redacts_*` зелёные).
- **Что видит пользователь:** новый ряд аккаунта (provider/label/authKind/scopes/expiry — БЕЗ ключа), форма сброшена. Ключ никогда не переотображается.
- **Дельта от ожидания:** нет. Маскирование честное.
- **Действие:** ничего. (Кросс-каттинг P-19: нет in-flight-гварда — двойной клик = два аккаунта; общая проблема всех диалогов.)

## J-04 — «Коннекторы»: «начать OAuth» (реестр провайдеров ПУСТ в v1)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor** (защитимо Important — фича 100% неработоспособна, но подана как доступная). Прямо соответствует открытому вопросу **O-5**.
- **Проверено (весь путь):**
  1. **Реестр пуст на буте:** `boot.rs:199-205` — комментарий «v1 boots with an EMPTY OAuth provider registry», `let connectors = Arc::new(ConnectorsState::new())`, БЕЗ `register_oauth_provider`. Греп по `crates/`+`src-tauri/`: `register_oauth_provider` вызывается ТОЛЬКО в `#[cfg(test)]` (accounts.rs:1038/1157/1201/1357) — ноль продакшн-сидинга. D14 (config-file registry) отложен.
  2. **Path клика:** `ConnectorsTab.tsx:294-313 handleBeginOAuth` → `connectorBeginOAuth({provider,label,scopes})` → wire `socket_server.rs:1693-1711` → `deps.connectors.begin_oauth(...)` → `provider_config` (accounts.rs:438-445) → `providers.lock().get(provider)` = None → `Err(ConnectorError::UnknownProvider(provider))`.
  3. **Маппинг:** `map_connector_err` (socket_server.rs:443-452), arm `other` → `Error{code: Io, message: "unknown OAuth provider: <X>"}` (Display из accounts.rs:122).
  4. **Фронт:** `describeOrchdError` (orchd.ts:777-778) code `Io` → **`ошибка сервиса: unknown OAuth provider: <X>`** → `showToast(...)`.
  - Тест `begin_oauth_unknown_provider_is_an_error` → **ok** (подтверждает UnknownProvider на незарегистрированном провайдере).
- **Обработка ошибок:** есть, честная в узком смысле — типизированная ошибка, не проглочена, не наврано. НО affordance — тупик.
- **Что видит пользователь:** после клика «начать OAuth» (с любой непустой строкой провайдера+метки) — 4-секундный toast **«ошибка сервиса: unknown OAuth provider: <то-что-набрал>»** (смешанный ru/en, эхо сырой строки). Challenge НЕ появляется, поля НЕ очищаются, кнопка снова активна. Кнопка «начать OAuth» отгейчена ТОЛЬКО `orchdDown || oauthBeginBlocked` (провайдер/метка пустые) — полностью активна в норме, БЕЗ пометки «скоро».
- **Дельта от ожидания / судейство:** каталог «Io „unknown provider"?» → подтверждено дословно. **Судейство (compare с precedent):** приложение уже использует паттерн «скоро» для неготового: ServersTab транспорт-пикер `stdio (скоро)`/`OAuth (скоро)` (disabled-опции), SkillsTab scope `проект (скоро)` (disabled). Секция «Подключить OAuth» НЕ применяет этот паттерн — рендерит полностью активную форму, которая в v1 не может дать успех, а копия «ошибка сервиса» читается как поломка сервиса, а не «ещё не подключено». Правильное решение (O-5): скрыть/пометить «скоро» всю секцию ЛИБО показать честный empty-registry стейт («нет настроенных OAuth-провайдеров»).
- **Действие:** UI-правка (Minor) + ответ владельца на O-5. Заодно `describeOrchdError` могла бы спец-кейсить UnknownProvider в человекочитаемое «OAuth-провайдер не настроен».

## J-05 — Challenge получен: вставить код → «завершить» (B-02: обмен без таймаута)

- **Вердикт:** 🟡 UX-GAP на уровне сценария (в v1 недостижимо). **B-02 = 🔴 Critical-класс латентный дефект, ПОДТВЕРЖДЁН статически, DORMANT в v1.**
- **Проверено (B-02 статически):** `accounts.rs:491-496 ssrf_guarded_http_client` ставит ТОЛЬКО `.redirect(Policy::none())` — НЕТ `.timeout()`/`.connect_timeout()`. Контраст: `GenericRestAdapter::new` (adapter.rs:109-114) `.timeout(GENERIC_REST_TIMEOUT=30s)`. Клиент используется в `complete_oauth` (accounts.rs:290-295 `request_async(&http).await`) И `refresh_oauth_token` (accounts.rs:408-412). ⇒ IdP token-эндпоинт, который принимает соединение, но не отвечает, вешает эти await бесконечно.
- **Усиление (то же, что F-03/B-01):** серверная диспетчеризация последовательна на соединение (`socket_server.rs` reader-loop, инлайн-await без per-request spawn) + один общий orchd-клиент → зависший `complete_oauth`-dispatch клинит ВЕСЬ orchd-конвейер. Клиентский `REQUEST_TIMEOUT=30s` (orchd_client.rs:61,387) лишь возвращает `Disconnected` (вводящая в заблуждение «оркестратор недоступен»), НЕ реконнектит; серверный клин держится до рестарта orchd.
- **НО в v1 НЕДОСТИЖИМО:** вся сетевая OAuth-поверхность мертва из-за пустого реестра (J-04): `begin_oauth` падает первым (UnknownProvider) → challenge не возвращается → блок с «завершить» (ConnectorsTab.tsx:577-608) не рендерится → `complete_oauth` не зовётся. Oauth-аккаунт создать нельзя → `refresh_oauth_token` (через expired-oauth в `token_for`) тоже недостижим. Дефект оживает В ТОТ ЖЕ МОМЕНТ, как зарегистрируют любой провайдер (D14 phase 3 / владелец).
- **«завершить» busy-стейт:** `handleCompleteOAuth` (ConnectorsTab.tsx:317-332) БЕЗ in-flight-гварда; кнопка `disabled={orchdDown || oauthCompleteBlocked}` — ни спиннера, ни «Завершение…». (Если бы путь был достижим: зависание → 30с → toast «оркестратор недоступен», challenge/код сохранены для ретрая — accounts.rs comment 329, но клиент врёт про причину, а сервер заклинен.)
- **P-06:** `oauth-code-input` (ConnectorsTab.tsx:589-596) НЕ имеет `disabled={orchdDown}` — только кнопка гейтится. Косметика: набрать в поле можно при down, но сабмит заблокирован (мутация не проходит). Честно на уровне мутации.
- **Дельта от ожидания:** каталог ждал «повесить IdP-стаб → диалог висит». Реально: в v1 недостижимо; но код-дефект реален и станет Critical при регистрации провайдера.
- **Действие:** BL-строка (🔴 Critical, но dormant): обернуть `complete_oauth`/`refresh_oauth_token` сетевой await в timeout ЛИБО задать `.timeout()` на `ssrf_guarded_http_client` (перенести D12-паттерн). Плюс busy-гвард на «завершить» и `disabled={orchdDown}` на code-input. Общий корень с F-03/B-01 (клиентский REQUEST_TIMEOUT должен помечать соединение мёртвым/реконнектить).

## J-06 — Аккаунт есть: ops runner (P-15: отказ listOps = пустой селект навсегда)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-15 подтверждён.)
- **Проверено:** `ConnectorsTab.tsx:260-269` useEffect: `connectorListOps({accountId}).then(ops => setOpsByAccount(...)).catch(e => showToast(describeOrchdError(e)))`. Гейт `if (account.id in opsByAccount) continue`. Рендер: `ops = opsByAccount[account.id] ?? []` (381); селект (422-438) — дефолт «— операция —» + опции по `ops`. Wire `socket_server.rs:1767-1779` → `connectors::adapter::list_ops(&account.provider)`.
- **Обработка ошибок:** отказ проглатывается в toast; `opsByAccount[id]` никогда не заполняется → селект пуст. **Retry нет:** deps useEffect — `[genericRestAccountIds]`, ре-ран только при смене набора id generic-rest-аккаунтов; в стабильном наборе повторного `connectorListOps` не будет. Неотличимо от «нет операций».
- **Что видит пользователь:** селект с единственной опцией «— операция —», args/«вызвать» disabled (`selected===""`), навсегда — до смены набора аккаунтов или рестарта. Нет ручного refresh, нет индикации «не загрузилось».
- **Смягчение (честно):** для ЕДИНСТВЕННОГО отгруженного адаптера (`generic-rest`) `list_ops` возвращает статичную пару get/post и почти неотказуема (падает лишь на отказе `get_account`/`NoAdapter`) — практический blast-radius в первой сессии мал. Но паттерн no-retry/empty==fail подтверждён.
- **Дельта от ожидания:** каталог/P-15 — «пустой селект навсегда, нет retry, нет отличия от нет-операций». Подтверждено.
- **Действие:** UI (Minor): при отказе `connectorListOps` держать per-account error-стейт + кнопку «повторить» (зеркально тому, чего не хватает research-пейну, F-11).

## J-07 — «Навыки»: выбрать SKILL.md → «+ навык» (P-16 + F-1)

- **Вердикт:** ✅ OK (поведение) + 🟡 Minor (P-16) + 📄 DOC-GAP (F-1).
- **Проверено:** `SkillsTab.tsx:178-205 handleAdd/handleDelete`: `skillAdd(name?, description?, mdPath, "global", null)` → на успехе reset + `refreshSkills`; на отказе `showToast(describeOrchdError(e))`. Wire `socket_server.rs:1818-1875` → `add_skill`/`delete_skill` → `map_err`. Валидация `skills/registry.rs`: `validate_md_path` (61-88) — относительный/несуществующий/симлинк-эскейп/директория → `Validation`; `add_skill` (196+) — если имя недоступно ни из аргумента, ни из frontmatter → `Validation("skill: name required (pass it explicitly or via the SKILL.md frontmatter)")`. orchdDown гейтит submit (258) и delete (305). Файлы-как-истина: `compute_file_state` → `Present`(no badge)/`Modified`(«изменён»)/`Missing`(«файл отсутствует»). Тесты `SkillsTab.test.tsx` — 16 зелёных.
- **Обработка ошибок:** есть, честная — валидация типизирована и её message показан («неверные данные: skill: name required …»). md_path через нативный пикер (`pickSkillFile`) — всегда существующий файл, так что относительный/missing нормально не возникают.
- **P-16 (подтверждён, негативно):** `pickSkillFile` (commands.rs:1144-1159) имеет ЕДИНСТВЕННЫЙ error-путь — `CommandError::Internal` («dialog channel closed», практически невозможно; cancel = `Ok(None)` → no-op). `handlePickFile` (SkillsTab.tsx:173-175) гонит его через `describeOrchdError`, который понимает лишь `kind:"daemon"/"disconnected"/"incompatibleOrchd"` (orchd.ts:762-792) → `internal` проваливается в финальный `return "неизвестная ошибка оркестратора"` (message `CommandError::Internal` теряется). Дефект маппинга реален, blast-radius ничтожный.
- **F-1 (адъюдикация):** отгруженный баннер `SkillsTab.tsx:209-211` = **«Навыки — это реестр; они исполняются, когда появится агент-оркестр (S6b).»**. `frontend-conventions.md:86-88` цитирует ЭТУ ЖЕ строку ДОСЛОВНО → **ПРАВ**. `architecture.md:530-531` цитирует **«Навыки исполняются, когда появится агент-оркестр (S6b) — сейчас это реестр»** — парафраз с другим порядком, которого в коде НЕТ → **ВРЁТ**. Правка: привести architecture.md к отгруженной строке (= frontend-conventions.md). (Смежно F-2: architecture.md:528 заявляет зеркало ruleset `Present/Modified/Missing`; skill-enum реально `Present/Modified/Missing` — registry.rs:136-145 — но по каталогу сам ruleset-enum `Ok/Missing/ExternallyModified`; отдельная находка, вне J-07.)
- **Что видит пользователь:** баннер «реестр/S6b» сверху (честно plumbing-only), новый ряд с именем из frontmatter (если не задано), бейдж «изменён»/«файл отсутствует».
- **Действие:** 📄 F-1: правка architecture.md:530. 🟡 P-16 (Minor): либо `pickSkillFile` возвращать понятную ошибку, либо в `handlePickFile` не гнать через `describeOrchdError`. Поведение add/delete — ничего.

## J-08 — «Журнал»: «задать лимит» (BL-78 + P-23)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `InvocationLog.tsx:173-190 handleSetPolicy`: `Number(spendCapUsd)`/`Number(ratePerMin)`; если `Number.isNaN` → `showToast("предел должен быть числом")` + `return` (177-180) — **toast-only, без inline**. На успехе reset + `refreshPolicies`; на отказе `showToast(describeOrchdError)`. Wire `socket_server.rs:1884-1906` → `upsert_policy` → на успехе push `PoliciesChanged`.
- **BL-78 (подтверждён):** таблица политик (244-269) — БЕЗ per-row delete-кнопки. Грепом по `crates/`+`src/`+`src-tauri/`: `DeletePolicy|RemovePolicy|delete_policy|TrustDeletePolicy|ResetPolicy` → **NONE FOUND**. Верба удаления/сброса политики НЕТ вообще. Оба лимита пустыми → upsert строки null/null (не удаление). Убрать политику из UI нельзя.
- **P-23 (подтверждён жёстко):** во всём `OrchdPush`-enum (orchd-proto/lib.rs, 16 вариантов: `*Changed`, `McpInvocationLogged`, `PoliciesChanged`, `ResearchRunsChanged`, …) НЕТ `AuditChanged`/`audit-changed`. Грепом по `crates/`+`src/`+`src-tauri/`: `AuditChanged|audit-changed|AuditRowsChanged` → **ничего**. Таблица «Аудит» рефетчится ТОЛЬКО на mount (`refreshAuditRows` в useEffect, 160-165). Инвокации живут по `orchd://mcp-invocation-logged`, политики — по `orchd://policies-changed`, но аудит — ни по чему. ⇒ после записи `policy_deny`/`connect allow` в `audit_log` таблица «Аудит» обновится лишь ремоунтом таба «Журнал».
- **Обработка ошибок:** есть, честная (non-число не долетает до вербы; серверная валидация scope/ref_id → `Validation`).
- **Что видит пользователь:** при не-числе — toast «предел должен быть числом», поля не сброшены; политика применяется через push (таблица политик — живая). Аудит — не живой.
- **Дельта от ожидания:** каталог — «не-число → toast без inline» (подтверждено), «BL-78 delete/reset нельзя» (подтверждено), «P-23 аудит не живой» (подтверждено).
- **Действие:** UI (Minor): inline-ошибка на не-число (зеркально `tool-call-error`); BL-78 — добавить верб+кнопку delete-policy; P-23 — добавить `AuditChanged`-пуш ИЛИ рефетчить аудит на `mcp-invocation-logged`/`policies-changed` (аудит-строки пишутся ровно этими путями).

---

## Сводка ключевого

1. **J-04 (OAuth пустой реестр) — 🟡 UX-GAP, главная находка эпика.** Реестр провайдеров ПУСТ на буте (`boot.rs:205`, ноль `register_oauth_provider` вне тестов; D14 отложен). Любая попытка «начать OAuth» → typed `UnknownProvider` → toast **«ошибка сервиса: unknown OAuth provider: <X>»**. Кнопка активна, БЕЗ «скоро» — гарантированный тупик с копией, читающейся как поломка сервиса. Precedent «скоро» уже есть в ServersTab/SkillsTab, но не применён. Это O-5: секцию OAuth стоит скрыть/пометить «скоро» / показать честный empty-registry.
2. **F-1 (баннер «Навыки») — 📄 DOC-GAP.** Отгруженная строка = `frontend-conventions.md:86-88` ДОСЛОВНО → этот док ПРАВ. `architecture.md:530` — парафраз, которого в коде нет → неверен, править architecture.md.
3. **B-02 (OAuth-обмен без таймаута) — 🔴 Critical-класс, но DORMANT.** `ssrf_guarded_http_client` без `.timeout()` (контраст adapter.rs:111 30s); `complete_oauth`/`refresh` могут вечно клинить весь orchd (как F-03/B-01). В v1 недостижимо (пустой реестр → нет challenge). Оживёт при первой регистрации провайдера.
4. **Кросс-каттинг подтверждён:** P-15 (J-06 listOps → пустой селект без retry), P-20 (J-02 нет пути к ConnectDialog — он в ServersTab единственном месте), P-23 (пуша аудита нет в proto вообще), P-16 (picker-error → generic message), BL-78 (нет delete/reset политики — нет верба), P-06 (code-input не гейтится, косметика).
5. **Хорошо (✅):** J-03 (add API-ключ — маскирование честное, Keychain-fail→Io, ключ для ретрая), J-07 поведение (валидация типизирована/показана, files-as-truth, orchdDown-гейт). J-01 — не баг (контролируемый чекбокс возвращается к серверному значению), лишь нет on-row сигнала.

**Не удалось проверить рантаймом:** реальный зависон живого IdP-token-эндпоинта для B-02 (нет стенда «принимает-но-не-отвечает») — вердикт статический (accounts.rs:491-496 без timeout + контраст adapter.rs:111 + архитектура клина из F-03). J-04/J-05 сетевые OAuth-пути в v1 недостижимы в принципе (пустой реестр), поэтому «что видит юзер при живом провайдере» смоделировано по коду, не прогнано. FE-компонентные тесты мокают `connectorBeginOAuth` на успех, так что empty-registry-путь доказан бэкендом (`begin_oauth_unknown_provider_is_an_error` ok + boot.rs), не FE-тестом.

# Эпик B — Workspace, файлы, терминал (B-01…B-14). Результаты инвестигейта

Репо: `/Users/sshlg/DATA/builder-pro-ai` (main, v0.7.0). READ-ONLY.
Тесты прогнаны:
`npx vitest run TerminalTabs FileTree FilePreview CommandStrip WorkspaceSidebar FilesRail HomeView`
→ **85 passed**;
`cargo test -p bpa-sessiond --lib remove_workspace_root` → **4 passed** (в т.ч.
`remove_workspace_root_last_one_is_rejected_with_last_root_code`);
`cargo test -p bpa-sessiond rehydrate` → **1 passed**
(`cold_rehydrate_then_attach_replays_persisted_marker_as_inactive`).

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| B-01 | 🟡 UX-GAP | Important | **P-03 подтверждён**: `WorkspaceSidebar.onAdd` (68-73) без try/catch — отказ `createWorkspace` = `void onAdd()` глотает reject → молчаливый no-op, ноль toast/навигации (контраст с `handleAttach`, у которого try/catch есть) |
| B-02 | ✅ OK | — | Отмена пикера: `pickFolder()===null → return` — чистый преднамеренный no-op, приемлемо |
| B-03 | ✅ OK | — | «+ Add root»: `FileTree.onAddRoot` (390-399) в try/catch → `showToast(describeCommandError)`; демон ре-валидирует путь (canonicalize/dup/escape); повторный тот же путь — идемпотентный no-op + broadcast `WorkspaceUpdated` |
| B-04 | 📄 DOC-GAP | Minor | **UI удаления root НЕ существует** (grep: `removeWorkspaceRoot` только в App/commands/events, ни одной кнопки). Verb+wrapper+бэкенд-guard `LastRoot` есть и тестируются, но сценарий UI-недостижим; будь он подключён — `describeCommandError` показал бы **сырой английский** `"cannot remove the last workspace root"` |
| B-05 | 🟡 UX-GAP | Important | **P-01 подтверждён**: `TerminalTabs.onNewTerminal` (28-47) без try/catch — `void onNewTerminal()` глотает reject `createSession` → молчаливый no-op; кнопка гейчена только на `!activeWorkspaceId`, НЕ на sessiond-down |
| B-06 | 🟡 UX-GAP | Important | **P-02 подтверждён**: `onClose` (49-52) без try/catch — reject `killSession` ПРОПУСКАЕТ `manager.dispose` (зомби-таб + течёт xterm) + reject проглочен. Плюс дельта: даже на успехе таб НЕ исчезает (`markExited` флипает в exited и висит; `removeSession` — мёртвый код) |
| B-07 | 🟡 UX-GAP | Important | Replay-only регидрат работает (тест зелёный). НО: (1) визуально живая/неактивная НЕ различимы — `StatusDot` игнорирует `isActive`; (2) ввод в неактивную сессию → `write_stdin`→`NoSuchSession`, а `term.onData(d=>void writeStdin())` (terminal-manager:147) без `.catch` → нажатия молча исчезают, ноль фидбека |
| B-08 | 🟡 UX-GAP | Minor | **P-12 частично**: пустая (treeCache=[]) и провал листинга различимы, НО обе кривые — пустая без явной «пусто»-метки; провал → `cacheDir` не зовётся → вечная строка «Загрузка…» (врёт про загрузку) + транзиентный toast + БЕЗ retry |
| B-09 | ✅ OK | — | Честные карточки: `Бинарный файл · size` / `Файл слишком большой… · size`; `PREVIEW_CAP=1 MiB` (fs_explorer:37); бинарь = NUL/invalid-UTF-8 в первых 8 KiB; TOCTOU-grow→TooLarge; отказ→карточка+toast+token-guard |
| B-10 | ✅ OK | — | `fs://watch-error`→`setWatchPaused(true)`→баннер; клик→`onRefreshWatch` fire-and-forget (**P-04** нет `.catch`, но `startWorkspaceWatch` по контракту не реджектит — ошибки идут через event); повторный отказ → баннер возвращается + toast листинга |
| B-11 | ✅ OK | — | `create_new`/guard перед `fs::rename` → `AlreadyExists` → «файл с таким именем уже существует»; пустое имя → тихая отмена (427); delete→Trash+confirm; всё в try/catch→toast |
| B-12 | 🟡 UX-GAP | Minor | **P-13 подтверждён (обе половины)**: загрузка и пустота одинаковы — «Пока нет команд»; отказ→`setFailed`→`return null` (рендер НИЧЕГО) навсегда, без retry-кнопки, только транзиентный toast; рефетч лишь на смену `sessionId`/`sessionMeta` |
| B-13 | 🟡 UX-GAP | Minor | «Пройти →» активирует workspace+сессию, вью=workspace, пейн монтируется→attach→replay (работает), порядок буккетов верный. НО клавиатурный ФОКУС в PTY не гарантирован на первом прыжке: `manager.focus` — no-op до `open()` (295-297), а `open()` не зовёт `term.focus()` (осознанно, док-коммент 288-292) |
| B-14 | ✅ OK | — | Клик по строке running/exited → `goTo(workspaceId, sessionId)` → навигация+активация сессии; тот же минорный focus-нюанс, что B-13, но фокус здесь не требуется |

**Итог по эпику:** 6×✅ OK · 7×🟡 UX-GAP (4 Important + 3 Minor) · 1×📄 DOC-GAP · 0×🔴 BUG.

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где |
|---|---|---|
| **P-01** («+ New terminal» silent no-op на reject) | 🟡 **ПОДТВЕРЖДЁН** (Important) | B-05 |
| **P-02** (close «×» reject → зомби-таб, пропущен `dispose`) | 🟡 **ПОДТВЕРЖДЁН** (Important) | B-06 |
| **P-03** («+ Add workspace» без try/catch) | 🟡 **ПОДТВЕРЖДЁН** (Important) | B-01 |
| **P-04** (watch-resume fire-and-forget без `.catch`) | ✅ **НЕ БАГ** (безвредно) | B-10 |
| **P-12** (пустая папка неотличима от провала) | 🟡 частично (различимы, но провал=вечная «Загрузка…») | B-08 |
| **P-13** (CommandStrip loading==empty + null навсегда) | 🟡 **ПОДТВЕРЖДЁН** (обе половины) | B-12 |

---

## Результаты

### B-01 — Sidebar: «+ Add workspace» → выбрать папку

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (P-03 подтверждён.)
- **Проверено:** `WorkspaceSidebar.tsx:68-73 onAdd` → `pickFolder()` (ipc/commands) → `createWorkspace(basename(dir), dir)` → `onSelectWorkspaceAndNavigate(ws.id)`. Кнопка `aria-label="Add workspace"` (275-291) вызывает `void onAdd()` (278). Бэкенд: `commands.rs::create_workspace` → `Request::CreateWorkspace` → sessiond валидирует root (spec §16) + push `workspace://created`.
- **Обработка ошибок:** **ОТСУТСТВУЕТ**, проглочена. `onAdd` НЕ обёрнут в try/catch. `pickFolder()===null` → `return` (чистая отмена, ок). Но если `createWorkspace` реджектит (`CommandError::Daemon`/`Disconnected` — невалидный/недоступный root, sessiond down), reject уходит в `void onAdd()` → **unhandled rejection, ноль фидбека**. Контраст в ТОМ ЖЕ файле: `handleAttach` (75-85) обёрнут в try/catch → `showToast(describeOrchdError)`; `onAdd` — нет.
- **Логи:** UI-слой ничего не эмитит. Демон — insert (класс B-04, пер-верб tracing нет).
- **Что видит пользователь:** при отказе — **ничего**: диалог папки закрылся, ни toast, ни новый workspace, ни навигация. Неотличимо от «передумал».
- **Дельта от ожидания:** каталог просил «воспроизвести ноль фидбека при отказе» — воспроизведено статически (нет catch → нет toast).
- **Действие:** BL/фикс (Important): обернуть `createWorkspace` в try/catch → `showToast(describeCommandError(e))`, зеркально `FileTree.onAddRoot` и `handleAttach`.

### B-02 — Пикер открыт: отменить пикер

- **Вердикт:** ✅ OK.
- **Проверено:** `WorkspaceSidebar.tsx:70` (`if (dir === null) return;`), аналогично `FileTree.tsx:393`. `pickFolder` (commands.ts:117) резолвит `string | null`; отмена → `null`.
- **Обработка ошибок:** н/д (отмена — не ошибка).
- **Что видит пользователь:** ничего не происходит — молчаливый no-op. Приемлемо (нет побочных эффектов, нет мутации).
- **Действие:** ничего.

### B-03 — Workspace открыт: FileTree «+ Add root» → папка

- **Вердикт:** ✅ OK.
- **Проверено:** `FileTree.tsx:390-399 onAddRoot` → `pickFolder()` → `addWorkspaceRoot(workspace.id, dir)` → `upsertWorkspace(ws)`. Кнопка `aria-label="Add root"` (671-688) → `void onAddRoot()`. Бэкенд `commands.rs:1048 add_workspace_root` → `Request::AddWorkspaceRoot` → sessiond ре-валидирует (canonicalize, отвергает дубли/escapes) + broadcast `WorkspaceUpdated`.
- **Обработка ошибок:** есть, честная. try/catch → `showToast('Не удалось добавить корень: ' + describeCommandError(err))` (397). `describeCommandError` (70-88) маппит `daemon/disconnected/internal/incompatibleDaemon/tooLarge`.
- **Логи:** UI toast; демон — insert.
- **Что видит пользователь:** второй root появляется в дереве (upsertWorkspace + push). Симлинки/сетевые тома/невалидный путь → `validate_dir` на демоне → `outsideRoot`/`io` → toast. **Повторное добавление того же пути:** демон идемпотентен (не пишет второй идентичный root, см. commands.rs:1041), возвращает тот же `Workspace` + harmless-resync broadcast — не ошибка.
- **Действие:** ничего.

### B-04 — Workspace с 2 roots: удалить root → LastRoot

- **Вердикт:** 📄 DOC-GAP. **Severity: Minor.**
- **Проверено:** grep `removeWorkspaceRoot`/`remove_workspace_root` по `src/` → совпадения ТОЛЬКО в `App.tsx`/`ipc/commands.ts`/`ipc/events.ts` (обёртка+тип), **ни одной кнопки/контрола** ни в `FileTree`, ни в `WorkspaceSidebar`, ни в `ProjectPanel`. FileTree предлагает только «+ Add root», удаления нет. Бэкенд-цепочка полностью существует и тестируется: `commands.rs:1065 remove_workspace_root` → `Request::RemoveWorkspaceRoot` → `persistence.rs:465 Err(PersistError::LastRoot)` (msg «cannot remove the last workspace root», код `"LastRoot"`) → `socket_server.rs:889` → `Response::Error{code,message}` → `err_from_response` (commands.rs:499) → `CommandError::Daemon{code:"LastRoot", message}`. Тест `remove_workspace_root_last_one_is_rejected_with_last_root_code` — зелёный.
- **Обработка ошибок:** на бэкенде честная (guard от 0 roots). На фронте — недостижима.
- **Что видит пользователь (гипотетически, будь UI):** `describeCommandError` для `kind:"daemon"` → `e.message` → **сырой английский** «cannot remove the last workspace root» (см. O-2 ru/en). Локализации нет.
- **Дельта от ожидания:** каталог ждал сценарий «удалить root → root исчез; последний → LastRoot; текст локализован/понятен». **Реально: UI удаления root в v1 отсутствует** (параллель с C-08 «архив UI-недостижим»); плюс латентная англ.-строка.
- **Действие:** DOC/BL: (a) зафиксировать в доках «удаление root в UI не реализовано (verb есть)»; (b) при добавлении UI — локализовать LastRoot-текст.

### B-05 — Workspace открыт: «+ New terminal»

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (P-01 подтверждён.)
- **Проверено:** `TerminalTabs.tsx:28-47 onNewTerminal` → root-aware cwd (39-43) → `createSession(activeWorkspaceId, opts)` (46). Кнопка (110-125): `disabled={!activeWorkspaceId}`, `onClick={() => void onNewTerminal()}`. Успех: `create_session` push `session://created` → App upsert+activate.
- **Обработка ошибок:** **ОТСУТСТВУЕТ**. `onNewTerminal` без try/catch, `await createSession` может реджектнуть (`Disconnected`, если sessiond отвалился; `Daemon`, если spawn упал) → reject уходит в `void onNewTerminal()` → **проглочен молча**. Гейт только `!activeWorkspaceId`, НЕ на sessiond-connection — при мёртвом sessiond кнопка активна, клик = тихий провал.
- **Логи:** ничего на UI; на успехе push, на провале — тишина.
- **Что видит пользователь:** при отказе — ничего: ни таба, ни toast. Молчаливый no-op на ключевом «time-to-first-terminal».
- **Дельта от ожидания:** каталог «Отказ → ?»; P-01 «молчаливый no-op» — подтверждено.
- **Действие:** BL/фикс (Important): try/catch → `showToast(describeCommandError(e))`.

### B-06 — Живая сессия: закрыть «×»

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (P-02 подтверждён + дельта.)
- **Проверено:** `TerminalTabs.tsx:49-52 onClose`: `await killSession(sessionId)` → `manager.dispose(sessionId)`. Кнопка «×» (88-105): `onClick` → `e.stopPropagation(); void onClose(s.id)`. Бэкенд `socket_server.rs:1031 KillSession` → SIGKILL (live) → wait-thread → push `session://exited` → store `markExited`; для INACTIVE — D3-путь (1033-1044): drop attach + remove map + delete rows.
- **Обработка ошибок:** **ОТСУТСТВУЕТ**. Нет try/catch. При reject `killSession` (напр. `Disconnected`): (1) `await` throw → **`manager.dispose` НИКОГДА не вызывается** (пропущен) → xterm-инстанс течёт; (2) throw уходит в `void onClose` → **проглочен** (нет toast); (3) `session://exited` не приходит (kill не прошёл) → таб остаётся ЖИВЫМ. Итог — **зомби-таб + утечка xterm + ноль фидбека**. Тест `closing a tab kills the session and disposes its terminal` покрывает только happy-путь (asserts killSession+dispose), reject-путь не покрыт.
- **Дельта от ожидания:** каталог «Сессия убита, таб исчез». **Реально даже на успехе таб НЕ исчезает:** `markExited` (store.ts:467-486) флипает `isActive=false`+`lifecycle=exited`, но НЕ удаляет из store; `removeSession` (440-448) — **мёртвый код** (ноль вызовов, нет push `session://removed`); `TerminalTabs` рендерит `Object.values(sessions)` без фильтра → убитая сессия висит как exited-таб, недемонтируемый из UI.
- **Логи:** ничего на UI-слое.
- **Что видит пользователь:** happy — таб становится exited-табом (не исчезает); reject — таб остаётся живым, xterm течёт, тишина.
- **Действие:** BL/фикс (Important): (a) `try { await killSession } finally { manager.dispose }` + catch→toast (чтобы dispose всегда шёл, а ошибка была видна); (b) решить судьбу exited-табов (prune через `removeSession` по push или ручное закрытие).

### B-07 — Живые сессии: перезапуск sessiond → attach (cold-rehydrate)

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.**
- **Проверено:** boot `boot.rs:115 cold_rehydrate_sessions` → `supervisor.rehydrate_inactive(meta, sb)` (pty_supervisor.rs:770): `is_active=false`, `pty=None`, ринг предзаполнен scrollback, `lifecycle` СОХРАНЯЕТСЯ как персистнутый (798). Attach INACTIVE-путь (attach.rs:171) → replay-only (`Push::Replay`), без live-reader. Тест `cold_rehydrate_then_attach_replays_persisted_marker_as_inactive` — зелёный. Реконнект: App list_sessions → upsertSession.
- **Обработка ошибок:** replay честный. Но два UX-провала:
  1. **Визуальная неразличимость live/inactive.** `StatusDot` (dotStateOf, StatusDot.tsx:13-26) смотрит ТОЛЬКО `lifecycle`+`waitingForInput`, **игнорирует `isActive`**. Сессия, персистнутая как `Running`/`AtPrompt`, после регидрата показывает зелёный «running»/idle-точку, хотя PTY мёртв. `TerminalTabs` не рендерит никакой inactive-метки. На Home такие (`!isActive` && lifecycle≠exited) выпадают из ВСЕХ трёх буккетов (`running` требует `isActive`; `exited` требует `lifecycle.exited`) → невидимы на Home, но висят «живым» табом в workspace.
  2. **Ввод в неактивную сессию — молча теряется.** `write_stdin`→`require_pty` (pty_supervisor.rs:555-559) для PTY-less entry → `SupervisorError::NoSuchSession` → `socket_server.rs:1018 err(...)`. Фронт: `term.onData((d) => void writeStdin(sessionId, d))` (terminal-manager.ts:147-148) — **fire-and-forget без `.catch`** → reject проглочен; локального эха нет (нет live-PTY) → **нажатия исчезают в никуда, ноль обратной связи**.
- **Что видит пользователь:** scrollback реплеится корректно (это ✅), НО не может отличить мёртвую сессию от живой и, набирая в неё, не получает ни символа, ни ошибки.
- **Дельта от ожидания:** каталог «Отличие живой/неактивной визуально понятно?» → нет; «Ввод в неактивную — что происходит?» → молчаливая потеря.
- **Действие:** BL/фикс (Important): (a) прокинуть `isActive` в `StatusDot`/таб (dim/бейдж «неактивна»); (b) `term.onData` → `writeStdin(...).catch(() => showToast('Сессия неактивна — перезапустите'))` или блокировать ввод для `!isActive`.

### B-08 — FileTree: раскрыть пустую папку

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-12 частично.)
- **Проверено:** `FileTree.tsx:117-181 computeFlatten` / эффект `273-288`. Раскрытая-но-некэшированная папка → синтетическая строка `loading` (129-142) + push в `pending`. Эффект: `listDir(...).then(cacheDir).catch(showToast).finally(delete key)` (278-285).
- **Обработка ошибок:** на провале `cacheDir` НЕ зовётся → `treeCache[key]` остаётся `undefined` → `computeFlatten` бесконечно эмитит `loading`-строку. Эффект НЕ ре-ранится (deps `[pending, showIgnored]`; `pending` — `useMemo` от `treeCache`, который не менялся) → **retry нет**, «Загрузка…» **навсегда**. Toast транзиентный (4с). Пустая папка: `listDir`→`[]`→`cacheDir(root,rel,[])`→`treeCache[key]=[]`→ ноль дочерних строк.
- **Что видит пользователь:** пустая = раскрытый узел ▾ **без единой строки и без явной «пусто»-метки** (каталог ждал «пусто»-индикацию — её нет); провал = **вечная «Загрузка…»** (врёт про загрузку) + мелькнувший toast, без retry. Во время in-flight обе одинаковы (корректно); терминально — различимы, но обе кривые.
- **Дельта от ожидания:** P-12 «неотличима» — буквально частично опровергнуто (различимы), НО провал маскируется под загрузку, а пусто без метки.
- **Действие:** BL/фикс (Minor): (a) явная строка «(пусто)» для `cached.length===0`; (b) при провале — сохранить honest error-строку + кнопку «повторить» вместо вечной «Загрузка…».

### B-09 — FileTree: открыть файл 2 МБ / бинарник

- **Вердикт:** ✅ OK.
- **Проверено:** `FilePreview.tsx:86-99` → `readFilePreview(root, rel)`; карточки: `binary` → «Бинарный файл · formatBytes» (122-124), `tooLarge` → «Файл слишком большой для предпросмотра · formatBytes» (126-132), error → error-карточка (110-116) + `showToast` (95). Бэкенд `fs_explorer.rs`: `PREVIEW_CAP = 1024*1024` (1 MiB, :37); stat>cap → `TooLarge` без чтения (350-351); read cap `.take(PREVIEW_CAP+1)` (355); бинарь = NUL/invalid-UTF-8 в первых `BINARY_PROBE_LEN`=8 KiB (:39, 295); TOCTOU-grow (`bytes.len()>CAP`) → `TooLarge` (326-327).
- **Обработка ошибок:** честная. Token-guard (`requestRef`, 72,81,88,92,98) от гонок re-select. `FilePreview`-тип различает `text|binary|tooLarge` на уровне типа → нельзя случайно отрендерить бинарь/обрезку как целый файл.
- **Логи:** UI toast; секретов нет.
- **Что видит пользователь:** честные placeholder-карточки с реальным размером; truncated-текст → баннер «Содержимое могло измениться…» (136-140).
- **Дельта:** нет. Границы капа — ровно 1 MiB, `size==CAP` → text, `CAP+1` → TooLarge (тесты `exact`/`oversized`).
- **Действие:** ничего.

### B-10 — Активный watch: удалить root на диске → «обновить»

- **Вердикт:** ✅ OK. (P-04 — не баг.)
- **Проверено:** `App.tsx:158 onFsWatchError(() => setWatchPaused(true))`; `FilesRail.tsx:157-177` рендерит баннер «live-обновления на паузе — обновить» при `watchPaused`; клик → `onRefreshWatch` (87-93): `void startWorkspaceWatch(roots, showIgnored)` + `invalidateDirs(root, ["*"])` + `setWatchPaused(false)`.
- **Обработка ошибок:** **P-04** — `startWorkspaceWatch` fire-and-forget без `.catch`, НО по контракту (`fs.ts:98-106`) он **никогда не реджектит**: любой сбой watch приходит как event `fs://watch-error`, не как reject промиса. ⇒ отсутствие `.catch` **безвредно**. При повторном отказе: оптимистичный `setWatchPaused(false)` на миг прячет баннер → `invalidateDirs` → FileTree ре-пуллит → `listDir` root'а падает → toast «Не удалось прочитать/обновить папку»; параллельно re-fired `fs://watch-error` → `setWatchPaused(true)` → баннер возвращается.
- **Что видит пользователь:** после повторного отказа — баннер снова + toast листинга (честная деградация). Мелкий флик баннера (off→on) — косметика.
- **Дельта:** нет.
- **Действие:** ничего (опц. косметика: не гасить баннер до успешного re-listing).

### B-11 — FileTree: создать/переименовать/удалить файл

- **Вердикт:** ✅ OK.
- **Проверено:** `FileTree.tsx`: `doCreate` (335-343)/`doRename` (345-357)/`doDelete` (359-372) — все в try/catch → `showToast('Не удалось …: ' + describeFsError)`. `submitForm` (422-431): `value.trim()===""` → тихая отмена (427). Бэкенд `fs_explorer.rs`: `create_file_inner` `.create_new(true)` (370), `rename_entry_inner`/`move` — guard `AlreadyExists` ДО `fs::rename` (395, 418). `describeFsError` (48-66): `alreadyExists`→«файл с таким именем уже существует», `notFound`/`permissionDenied`/`outsideRoot`/`tooLarge`/`io`. Delete → Trash (доверенный `deleteEntry`) + `window.confirm` (361). Тесты (по именам): `create_file_does_not_overwrite_existing`, `rename_entry_onto_existing_target_is_rejected_without_overwriting`, `rename_entry_onto_a_free_name_still_succeeds`.
- **Обработка ошибок:** честная; конфликт имён — код `create_new`/pre-rename-guard, текст «…уже существует».
- **Что видит пользователь:** ряд появился/переименован/уехал в корзину; существующее имя → toast «…уже существует»; пустое имя → тихая отмена.
- **Действие:** ничего.

### B-12 — Открыта сессия: прогнать команды (OSC-133) → CommandStrip

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-13 подтверждён.)
- **Проверено:** `CommandStrip.tsx:141-157` — `getCommandEvents(sessionId, 10)`; `pairCommandEvents` (45-72) → чипы `outcome`(✓/`✗ code`)/`running`/`interrupted` (honest-state: lone `started` на `!isLive` → «прервано», 192-210). Рефетч на `[sessionId, sessionMeta, showToast]`.
- **Обработка ошибок:** **P-13 (обе половины)**: (1) **loading==empty** — при in-flight `events=[]` → `items.length===0` → «Пока нет команд» (170-171), идентично истинной пустоте; (2) **провал → null навсегда**: `.catch`→`setFailed(true)`+toast (152-156), затем `if (failed) return null` (160) → рендерит **НИЧЕГО**, без retry-кнопки; повторный фетч только при смене `sessionId`/`sessionMeta` (lifecycle-push), при стабильной сессии — полоса пропадает молча.
- **Логи:** UI toast «Не удалось загрузить историю команд».
- **Что видит пользователь:** чипы корректны; но «Пока нет команд» неотличимо от загрузки; при сбое — пустое место + один транзиентный toast.
- **Дельта:** каталог/P-13 подтверждён.
- **Действие:** BL/фикс (Minor): отдельный loading-стейт (не «Пока нет команд»); при `failed` — inline «не удалось загрузить · повторить» вместо `null`.

### B-13 — Home, ≥1 «нужен ты»: «Пройти →»

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `HomeView.tsx:172-179 goTo(workspaceId, sessionId)`: `setActiveWorkspaceId` → `setView("workspace")` → `setActiveSession(sessionId)` → `manager.focus(sessionId)`. Порядок буккетов: `waiting`(«Нужен ты», 250) → `running`(«Работают», 289) → `exited`(«Завершились недавно», 321) — совпадает с каталогом. Пейн монтируется → `TerminalPane` effect → `ensure/attach/open` → replay.
- **Обработка ошибок:** н/д (навигация локальная).
- **Что видит пользователь:** прыжок в workspace, сессия активна, scrollback на месте. НО **клавиатурный фокус в PTY НЕ гарантирован** на первом прыжке: `manager.focus` (terminal-manager.ts:295-297) — no-op, если `entry.opened===false`; `open()` (497-523) НЕ зовёт `term.focus()`. Т.е. на свежую (ни разу не открытую) сессию фокус НЕ ставится — юзеру надо кликнуть в терминал. Ограничение осознано (док-коммент 288-292: «focus() merely saves a click for a session whose pane was already opened»).
- **Дельта от ожидания:** каталог «фокус в PTY» — выполняется только для ранее открытых пейнов; на первом прыжке — нет.
- **Действие:** BL/фикс (Minor): фокусировать `term` в `open()` (или в mount-effect `TerminalPane`), когда сессия только что стала активной.

### B-14 — Home: клик по строке running/exited

- **Вердикт:** ✅ OK.
- **Проверено:** `HomeView.tsx`: running-строка `onClick={() => goTo(meta.workspaceId, meta.id)}` (307), exited-строка (342) — тот же `goTo`. Навигация: `setActiveWorkspaceId`+`setView("workspace")`+`setActiveSession`. Групповой заголовок → `goTo(group.workspaceId)` (без сессии) → переход в workspace без выбора сессии.
- **Обработка ошибок:** н/д.
- **Что видит пользователь:** переход в workspace, сессия активна (для exited — открывается её exited-пейн со scrollback). Тот же минорный focus-нюанс, что B-13, но клик по running/exited не подразумевает немедленного набора.
- **Действие:** ничего (focus-улучшение общее с B-13).

---

## Сводка ключевого

1. **Триада «молчаливого no-op» (P-01/P-02/P-03) подтверждена** — три параллельных места без try/catch на fire-and-forget-мутациях: создание workspace (B-01), новый терминал (B-05), закрытие сессии (B-06). Все три глотают reject через `void handler()`, ноль toast. B-06 хуже прочих: reject ещё и пропускает `manager.dispose` (утечка xterm + зомби-таб). Все — Important, тривиальный фикс (обернуть в try/catch → `describeCommandError`/finally-dispose).
2. **B-07 cold-rehydrate — Important UX-провал** на двух фронтах: `StatusDot` игнорирует `isActive` (мёртвая сессия выглядит живой), а ввод в неактивную сессию (`write_stdin`→`NoSuchSession`) молча теряется из-за `void writeStdin` без `.catch`. Сам replay-only регидрат корректен (тест зелёный).
3. **B-04 — UI удаления root отсутствует** (verb+бэкенд+тесты есть, кнопки нет) — прямая параллель C-08; плюс латентная сырая англ.-строка LastRoot (O-2).
4. **Мелочи (Minor):** B-08 (провал листинга = вечная «Загрузка…» без retry, пусто без метки), B-12/P-13 (loading==empty; сбой→null навсегда), B-13 (фокус в PTY не гарантирован на первом прыжке).
5. **Хорошо (✅):** B-02 (отмена пикера), B-03 (add root — try/catch+ре-валидация), B-09 (честные карточки binary/tooLarge, cap 1 MiB, token-guard), B-10 (watch-error→баннер, P-04 безвреден), B-11 (CRUD-конфликты честны), B-14 (навигация).
6. **P-04 — НЕ баг:** `startWorkspaceWatch` по контракту не реджектит (сбои идут через `fs://watch-error`), поэтому отсутствие `.catch` в `onRefreshWatch` безвредно.

**Не удалось проверить рантаймом:** тесты crate `builder-pro-ai` (Tauri-core: `fs_explorer.rs`/`commands.rs`) **не компилируются в этом checkout** — build.rs требует sidecar-бинарь `binaries/bpa-orchd-aarch64-apple-darwin`, которого нет (`resource path … doesn't exist`). Вердикты B-09/B-11/B-04-mapping построены на исходниках + именах существующих `#[cfg(test)]`-тестов (греп), не на их прогоне. Реальный ввод в живую inactive-сессию и визуальная неразличимость (B-07) проверены статически (нет запущенного демона/GUI-стенда — по ограничению READ-ONLY, без касания launchd/демонов).

# Эпик I — Граф (I-01…I-09). Результаты инвестигейта

Репо: `/Users/sshlg/DATA/builder-pro-ai` (main, v0.7.0). Read-only.
Модель: opus. Пути прослежены: UI-контрол → ipc-wrapper → Tauri command → wire verb → dispatch → `graph.rs`.
Тесты прогнаны:
`npx vitest run src/components/graph/GraphCanvas.test.tsx src/components/graph/graphMapping.test.ts src/components/graph/nodeRenderers.test.tsx` → **37 passed**;
`cargo test -p bpa-orchd --lib graph` → **56 passed** (вкл. `research::graph_ingest_tests::*`).

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| **I-01** | **🟡 UX-GAP** | **Important** | **P-22 подтверждён: узел создаётся hardcoded «Новый узел» + body="" , формы title/body НЕТ, rename НЕТ, edge-label/edge-kind edit НЕТ. `orchdGraphUpdateNode` (+Tauri command +wire verb +`update_node`) существуют end-to-end, но НИ ОДНОГО call-site в `src/`. Человеко-facing редактор знаний — фактически стаб: можно расставить безымянные типизированные коробки и соединить их «relates», но выразить знание нельзя. Код соответствует §7 (форма никогда не специлась) → DOC-GAP vs собственная цель спека «editable graph». O-7.** |
| I-02 | 🟡 UX-GAP | Minor | P-08: optimistic edge add без rollback; при отказе — toast, но фантомное ребро (client-id) висит до следующего `refreshGraph` (ремоунт таба / чужой push / reconnect); нет polling. Плюс: drag-ребра при `orchdDown` → `onConnect` early-return БЕЗ per-action фидбека (только глобальный OrchdDownBanner). |
| I-03 | 🟡 UX-GAP | Minor | P-07: flush move при `orchdDown` молча дропается (`return` без toast/revert). Узел визуально остаётся на несохранённой позиции (выглядит «сохранено») до следующего `refreshGraph`, затем СНАП назад к серверной позиции (на ремоунте таба / `onOrchdUp`-reconnect). Худший вариант из двух: сначала «как сохранилось», потом тихий откат. |
| I-04 | ✅ OK | — (Minor) | Частичный отказ delete-цикла: `finally refreshGraph` доводит канву до серверной истины (удалённые до отказа id не висят). Состояние консистентно. Minor-оговорка: toast generic (не перечисляет что/сколько упало); co-select узла + его инцидентного ребра → cascade удаляет ребро, `deleteEdge` ловит `NotFound` → ложный toast «не найдено» при фактическом успехе. |
| I-05 | ✅ OK | — (Minor) | Stale-response guard (`searchRequestIdRef` монотонный, bump на dispatch И на clear) присутствует + покрыт тестом. Input живой (read, не гейтится). Minor-оговорка: сам вызов `orchdGraphSearch` при `orchdDown` падает (IPC-read нужен orchd) → toast-ошибка, подсветка офлайн не работает. |
| I-06 | ✅ OK | — | Ghost (external) клик → `openProject(data.projectId)` (FOREIGN project из `graphMapping`). Покрыт тестом. |
| I-07 | 🟡 UX-GAP | Minor | Локальный (не-external) клик = осознанный no-op с ZERO фидбеком. При этом `nodeCardStyle` ставит `cursor:"pointer"` ВСЕМ узлам → pointer-курсор обещает кликабельность, а клик ничего не делает → нечестный affordance. Спека §7 сама говорила «entityRef click → navigate to the entity»; код отложил (документированный follow-up, «no deep-link seam»). Spec-deviation. |
| I-08 | ✅ OK | — | Orphan «источник удалён»: `is_orphan` вычисляется server-side в `resolve_node_label` НА READ-TIME (`list_project_graph`), когда `resolve_entity_label`→`None`. `EntityRefNode` рендерит `data.isOrphan ? "источник удалён" : data.label`. Покрыт тестом `nodeRenderers`. |
| I-09 | 🟡 UX-GAP | Minor | Self-loop → `Invariant`, дубль → `Conflict`, оба → toast. Тексты честны, но mixed ru/en с сырым английским + UUID: self-loop = «недопустимая операция: graph edge source and target must differ (no self-loops)»; дубль = «конфликт: edge {uuid}->{uuid} (relates) already exists». Достижимо из UI (drag handle→handle). Плюс: отклонённое ребро остаётся фантомом на канве (см. I-02). O-2 ru/en. |

**Итог по эпику:** 4×✅ OK · 5×🟡 UX-GAP (1 Important + 4 Minor) · 0×🔴 BUG.
Главная находка — **I-01/P-22**: редактор графа как инструмент авторинга знаний — стаб (нет ни имени узла, ни тела, ни rename, ни label ребра), при том что весь backend-путь `update_node` готов и не подключён.

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| **P-22** (узел hardcoded «Новый узел», нет формы/rename/edge-label; `orchdGraphUpdateNode` обёртка есть) | **🟡 Important — ПОДТВЕРЖДЁН** | I-01 |
| P-08 (optimistic edge add без rollback; фантом до push) | 🟡 Minor — подтверждён | I-02 |
| P-07 (move-flush при orchdDown молча теряется) | 🟡 Minor — подтверждён | I-03 |
| B-09 (`GraphAddEdge` post-insert lookup-fail → push молча пропущен) | ✅ приемлемо (путь недостижим на практике) | I-02 (прим.) |

---

## Результаты

### I-01 — Таб «Граф»: выбрать kind → «Добавить» (P-22 — is the editor usable?)

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.** (P-22 подтверждён; DOC-GAP vs DoD «editable graph»; O-7.)
- **Проверено:** код-путь `GraphCanvas.tsx:436-444` (`handleAddNode`) → `orchdGraphAddNode(projectId, addKind, NEW_NODE_LABEL, "", posX, posY)` (`GraphCanvas.tsx:439`; `NEW_NODE_LABEL = "Новый узел"` — `GraphCanvas.tsx:58`; body — литерал `""`) → `orchd.ts:344-353` → Tauri `orchd_graph_add_node` → `socket_server.rs:1261-1280 GraphAddNode` → `graph.rs:343-380 add_node`. После успеха — `refreshGraph(projectId)` (`:440`). Тест `'toolbar "Добавить" calls orchdGraphAddNode with the selected kind'` (GraphCanvas.test.tsx:297) — зелёный.
- **Обработка ошибок:** есть, честная. `try/catch` → `showToast(describeOrchdError(e))` (`:441-443`). Add-node select+кнопка `disabled={orchdDown}` (`:483, 496`). Kind-select исключает `entityRef` (`ADDABLE_KINDS`, `GraphCanvas.tsx:56`; тест :316).
- **Отсутствие формы/rename (ядро P-22):**
  - `handleAddNode` НЕ открывает форму/prompt — сразу `orchdGraphAddNode` с константой «Новый узел» и пустым телом. Позицию считает `nextNewNodePosition` (grid).
  - **Rename после создания:** ни `onNodeDoubleClick`, ни inline-`<input>` в `DomainNode`/`EntityRefNode` (оба рендерят статичные `<div>`, `GraphCanvas.tsx:216-241`). Единственный node-handler — `onNodeClick` (клик = навигация/no-op, I-06/I-07).
  - **`orchdGraphUpdateNode` существует, но не подключён:** обёртка `orchd.ts:355-361` + Tauri `commands.rs:2002` + wire `GraphUpdateNode` + `graph.rs:442-464 update_node` — весь путь готов. Grep call-site в `src/` (минус `.test.`) → **ноль**. Т.е. переименование узла (title) И правка body из UI невозможны.
  - **Edge-label / edge-kind:** `onConnect` жёстко `orchdGraphAddEdge(source, target, "relates", "")` (`GraphCanvas.tsx:417`) — все рёбра «relates» с пустым label; UI сменить kind/label ребра не даёт (`update_edge` вообще нет ни в верб-списке §3, ни в `graph.rs`).
- **S4 spec §7 — специлась ли форма title?** НЕТ. §7 (`spec:192`): «a small toolbar (add node of a chosen kind, delete selected node/edge, a search box → orchdGraphSearch …); entityRef nodes click → navigate…». Ни title/body-формы, ни rename-контрола, ни edge-label-редактора в контракте UI нет. То есть **код соответствует §7** — но §7 внутренне недоспецифицирован против собственной цели спека («typed nodes … editable», §0, и DoD «the graph is editable in the UI», §строка 9). DOC-vs-CODE: DoD обещает «editable», §7 не даёт способа задать/сменить смысл узла.
- **Логи:** FE — toast. Демон `add_node` — insert без per-verb tracing (системное B-04). No-secrets покрыт `no_secrets_in_logs_graph.rs`.
- **Что видит пользователь первой сессии:** открывает «Граф», видит авто-seeded strategic-goal entityRef (server-side, D6). Жмёт «Добавить» → появляется коробка «Новый узел» с типом-плашкой. Ещё раз → вторая «Новый узел». Ни одну переименовать/описать нельзя; все они визуально идентичны. Соединить их можно только ребром «relates» без подписи. **Осмысленный граф знаний собрать нельзя.**
- **Адъюдикация «рабочий ли редактор»:** как ВЬЮЕР server-populated ref-узлов (strategic goal, insight-accept D9) — да, работает (I-08 orphan, I-06 ghost-nav). Как человеко-facing РЕДАКТОР авторинга знаний — **нет, это скелет/стаб**: примитивы add/move/delete/connect есть, но выразить контент (имя, тело, тип/подпись связи) невозможно, и backend `update_node` готов, но не проведён в UI.
- **Дельта от ожидания:** каталог спрашивал «рабочий ли вообще граф-редактор» → как редактор знаний — нет.
- **Действие:** BL (Important) + **эскалировать O-7** (заготовка под S6 или дефект v1?): подключить `orchdGraphUpdateNode` к inline-rename (double-click на узле) + форму title/body в add-node; добавить edge-label/kind edit (нужен `GraphUpdateEdge` — сейчас его нет). Минимум для «editable» DoD — переименование узла.

### I-02 — Два узла: протянуть ребро (optimistic add, P-08)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-08 подтверждён.)
- **Проверено:** `GraphCanvas.tsx:413-422 onConnect`: `if (useAppStore.getState().orchdDown) return;` (`:415`, fresh-read) → `setEdges((eds) => addEdge(connection, eds))` (`:416`, optimistic, client-id) → `orchdGraphAddEdge(source, target, "relates", "").catch(showToast(describeOrchdError))` (`:417-419`). **НЕТ** rollback в `.catch`, **НЕТ** явного `refreshGraph` (осознанно — doc-comment `:267-273`: полагается на `orchd://graph-changed` push). Backend: `socket_server.rs:1307-1337 GraphAddEdge` → на успехе push `GraphChanged` обоим эндпоинт-проектам; **на отказе → `map_err`, push НЕТ** (spec §6: «Failed verb → no push»). Тест `'a failed orchdGraphAddEdge call shows the mapped error via a toast'` (GraphCanvas.test.tsx:475) — зелёный.
- **Обработка ошибок:** отказ → toast (честно), но локальное ребро НЕ откатывается.
- **Сколько живёт фантом:** до следующей замены store-`view` (→ `useEffect [view]` пере-деривит local `edges`, `GraphCanvas.tsx:324-332`), т.е. до следующего `refreshGraph`. На пути ОТКАЗА push не приходит → фантом висит до: (a) ремоунта таба «Граф» (уход и возврат → mount-effect `refreshGraph`, `:316-319`; ProjectPanel рендерит таб условно — `ProjectPanel.tsx:428` — так что смена таба реально анмоунтит), (b) любой ДРУГОЙ успешной граф-мутации в этом проекте (её push → `refreshGraph`), (c) reconnect `onOrchdUp` (`App.tsx:273`). Polling НЕТ. Если юзер остаётся на табе и ничего не делает — фантом висит бессрочно (с client-id, выглядит как реальное ребро; toast к тому времени истёк, P-21 4с).
- **orchdDown-гейт (P-08 «early-return без фидбека»):** handles не гейтятся (`nodesConnectable` не выставлен → default true), drag-коннект физически возможен при `orchdDown`; `onConnect` `return` на `:415` БЕЗ добавления ребра и БЕЗ toast → per-action ноль фидбека. Смягчение: `ProjectPanel.tsx:283` рендерит `<OrchdDownBanner/>` при `orchdDown` (глобальный контекст «оркестратор недоступен» виден). Тест `'onConnect does nothing while orchdDown'` (GraphCanvas.test.tsx:238) — зелёный.
- **Логи:** FE — toast. `lifecycle`/insert — B-04.
- **B-09 (смежно):** `socket_server.rs:1322-1331` — если `edge_endpoint_projects` упадёт СРАЗУ после успешного insert, push пропускается молча (только `tracing::error!`, ответ успешен). Путь помечен «Unreachable in practice» (ребро только что вставлено под тем же сериализующим `db`-guard). Приемлемо — недостижим.
- **Что видит пользователь:** happy — ребро появляется, push реконсилит серверный id. Fail (self-loop/dup — I-09) — toast + ребро визуально ОСТАЛОСЬ (как и предсказывал каталог «ребро осталось визуально»).
- **Дельта от ожидания:** совпадает с P-08. Каталог «Отказ → toast, ребро осталось визуально» — подтверждено; уточнение: живёт до ремоунта/чужого push/reconnect.
- **Действие:** BL (Minor): в `.catch` откатывать локальное ребро (убрать client-id edge) ИЛИ явный `refreshGraph` на отказе; для orchdDown-drag — короткий toast «оркестратор недоступен».

### I-03 — Узел: drag (debounce 400ms; orchdDown в момент flush, P-07)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (P-07 подтверждён.)
- **Проверено:** `GraphCanvas.tsx:393-407 onNodesChange` — `applyNodeChanges` локально сразу (drag живой) + буфер moves, debounce `MOVE_DEBOUNCE_MS=400` (`:45`). `flushMoves` (`:378-391`): `dedupeMovesById` (последняя позиция на id) → **`if (useAppStore.getState().orchdDown) return;`** (`:385`, fresh-read) → иначе `orchdGraphMoveNode(...).catch(toast)`. Ранний `return` — БЕЗ toast, БЕЗ revert, БЕЗ ре-flush. Тест `'the move-flush does NOT call orchdGraphMoveNode while orchdDown'` (GraphCanvas.test.tsx:267) — зелёный.
- **Обработка ошибок:** при orchdDown — тихий дроп (нет честной деградации на per-action уровне; глобальный OrchdDownBanner присутствует). Node draggable при down (`nodesDraggable` не выставлен → default true; drag НЕ гейтится, гейтится только flush).
- **Снап назад или зависание — трейс:** local `nodes` уже несёт новую позицию (applyNodeChanges). Store-`view` не менялся (move не ушёл → сервер не менялся → push нет). Local пере-деривится из `view` только при смене `view` (`useEffect [view]`, `:324-332`). Значит узел ВИЗУАЛЬНО остаётся на несохранённой позиции (выглядит «сохранено»), пока `view` не заменится следующим `refreshGraph`: ремоунт таба (`ProjectPanel.tsx:428` + mount-effect) ИЛИ `onOrchdUp`-reconnect (`App.tsx:273` — граф рефетчится, в отличие от research runs F-10). Тогда `view` заменяется → узел **СНАП назад к серверной (старой) позиции**. То есть: сначала «как будто сохранилось», затем тихий откат при следующем refresh.
- **Что хуже:** именно этот вариант (тихо «сохранилось» → потом молча откатилось) хуже немедленного снапа: юзер уверен, что позиция записана, и теряет её без единого сигнала. Позиция «permanently lost» — да (move так и не отправлен), но канва самолечится к серверной истине на reconnect (честно к БД, неожиданно для юзера).
- **Логи:** нет (ранний `return`).
- **Дельта от ожидания:** каталог «orchdDown в момент flush → потеряно молча» — подтверждено; уточнение: не снап сразу, а «висит несохранённым → снап назад на следующем refresh».
- **Действие:** BL (Minor): при `orchdDown` в `flushMoves` — toast «позиция не сохранена: оркестратор недоступен» (или отложенный ре-flush после `onOrchdUp`). Сейчас — тихая потеря.

### I-04 — Выбраны узлы/рёбра: «Удалить выбранное» (частичный отказ)

- **Вердикт:** ✅ OK. (Minor-оговорки ниже.)
- **Проверено:** `GraphCanvas.tsx:446-463 handleDeleteSelected`: собрать `selectedNodeIds`/`selectedEdgeIds`; пусто → no-op (`:449`); `window.confirm(DELETE_CONFIRM_TEXT="удалить выбранное?")` (`:450, :62`); `try { for id of nodes await deleteNode; for id of edges await deleteEdge } catch { toast } finally { await refreshGraph }` (`:451-462`). Тесты: confirm-гейт (:365), пусто→no-op (:386), **`'a partial multi-delete (2nd id rejects) still deletes the 1st, toasts, AND reconciles via refreshGraph'`** (:397) — все зелёные.
- **Состояние после частичного отказа — консистентно?** ДА. При отказе на i-м id цикл бросает → `catch` toast → `finally refreshGraph(projectId)` доводит канву до серверной истины (id, удалённые до отказа, не висят; неудалённые остаются). Это фикс T7 review #3 (doc-comment `:456-461`); `refreshGraph` глотает свои ошибки в toast, так что `await` в `finally` не пробрасывает.
- **Обработка ошибок:** есть, честная (toast + реконсиляция). Minor: toast generic — не сообщает, ЧТО/сколько упало (частичность невидима сверх «была ошибка»).
- **Minor-оговорка (co-select node+его ребро):** выбрать узел A и инцидентное ребро A-B, удалить: `deleteNode(A)` каскадит ребро A-B (FK ON DELETE CASCADE, `graph.rs:490-500`), затем `deleteEdge(edgeAB)` → `edge_endpoint_projects` → `NotFound` → цикл бросает → ложный toast «не найдено» ПРИ фактическом полном успехе. Канва при этом консистентна (оба ушли). Сбивающий, но безопасный.
- **Логи:** FE toast; delete — B-04.
- **Что видит пользователь:** confirm → удаление; при серверном отказе части — «не найдено»/иная ошибка + канва сведена к правде.
- **Дельта от ожидания:** каталог «Состояние после частичного отказа консистентно?» → ДА. Оговорки — Minor.
- **Действие:** ничего блокирующего. Опц. Minor-BL: не бросать на `NotFound` в delete-цикле (idempotent-delete) — убрать ложный co-select toast; агрегировать ошибки цикла в один осмысленный итог.

### I-05 — Граф: поиск (живо при orchdDown; stale-response guard)

- **Вердикт:** ✅ OK. (Minor-оговорка.)
- **Проверено:** `GraphCanvas.tsx:337-366` debounce-search: пустой query → bump `searchRequestIdRef` + `setMatchIds(new Set())` (инвалидирует in-flight, `:345-347`); иначе `setTimeout(SEARCH_DEBOUNCE_MS=400)` → `requestId = ++searchRequestIdRef` → `orchdGraphSearch(q, projectId).then(if requestId===current setMatchIds).catch(if current showToast)` (`:349-361`). Монотонный `searchRequestIdRef` (`:312`) bump на КАЖДОМ dispatch И на clear → старая резолюция не перезатрёт свежую. Тест **`'ignores a STALE search response: fire A then B, resolve B then A -> matches reflect B'`** (GraphCanvas.test.tsx:439) — зелёный; `'a search query debounces then calls orchdGraphSearch(query, projectId)'` (:420) — зелёный. Backend `graph.rs:812-852 search_nodes` (LIKE label/body, cap 200).
- **Обработка ошибок:** есть, честная. Stale-drop и в `.then`, и в `.catch`. Input НЕ гейтится `orchdDown` (read-дисциплина, как RulesetPanel).
- **Minor-оговорка «живо при orchdDown»:** input живой (можно печатать), НО сам `orchdGraphSearch` — IPC-read, требует orchd; при `orchdDown` промис отклоняется → `.catch` → toast-ошибка, подсветки нет. То есть «живо» = input не disabled; фактическая подсветка офлайн невозможна (round-trip к серверу).
- **Логи:** FE — toast на ошибке. `search_nodes` — B-04.
- **Что видит пользователь:** ввод → через 400мс подсветка совпадений (`boxShadow` на match, `nodeCardStyle` `:198`); очистка мгновенно снимает подсветку.
- **Действие:** ничего.

### I-06 — Ghost-узел (другой проект): клик → переход в тот проект

- **Вердикт:** ✅ OK.
- **Проверено:** `GraphCanvas.tsx:424-434 onNodeClick`: `if (data.isExternal) { openProject(data.projectId); return; }` (`:427-430`). `data.projectId` для external — FOREIGN project (`graphMapping.ts:38-43, 90-108` — `toFlowNodes` мапит `externalNodes` с `isExternal:true` и их СОБСТВЕННЫМ `projectId`). Backend наполняет `external_nodes` в `list_project_graph` (`graph.rs:683-699`). Тест `'ghost (external) node click navigates via openProject(ghostProjectId)'` (GraphCanvas.test.tsx:324) — зелёный.
- **Обработка ошибок:** н/д (навигация — синхронный `openProject`).
- **Что видит пользователь:** клик по dimmed/dashed ghost-узлу (`opacity:0.6, borderStyle:dashed`, `nodeCardStyle:199-200`) → переключение на его проект.
- **Действие:** ничего.

### I-07 — Локальный entity_ref: клик → честный no-op (ноль фидбека — приемлемо?)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (Spec-deviation §7.)
- **Проверено:** `GraphCanvas.tsx:424-434 onNodeClick`: ветка `isExternal` навигирует; иначе — падение сквозь функцию, комментарий `:431` «Local entityRef click: honest no-op MVP». Тест `'local entityRef node click does NOT call any mutating wrapper or crash (honest no-op MVP)'` (GraphCanvas.test.tsx:334) — зелёный.
- **Ноль фидбека — честно ли?** Нет, частично нечестно: `nodeCardStyle` (`GraphCanvas.tsx:190-204`) ставит **`cursor:"pointer"` ВСЕМ узлам** (`:203`) — pointer-курсор обещает кликабельность, а клик по локальному узлу (entityRef ИЛИ обычному concept/fact/…) не делает ничего и ничего не сообщает. Affordance вводит в заблуждение.
- **Spec vs code:** §7 (`spec:192`) специл «entityRef nodes click → navigate to the entity (switch ProjectPanel tab / openProject)». Код это НЕ реализовал — отложил в no-op (doc-comment `GraphCanvas.tsx:275-282`: нет deep-link-seam в конкретную строку Цели/Идеи/Задачи/Инсайты; «faking a navigation … worse UX than nothing»; трекается как follow-up). То есть no-op — не «спека так велела», а осознанное отступление от §7 (документированное).
- **Обработка ошибок:** н/д.
- **Что видит пользователь:** кликает узел с pointer-курсором → ничего. Для entityRef (ссылка на реальные goal/idea/insight/task этого проекта) ожидал бы перехода.
- **Дельта от ожидания:** каталог/§7 ждали навигацию; реально — молчаливый no-op с обещающим курсором.
- **Действие:** BL (Minor): либо реализовать deep-link (§7), либо честный affordance — `cursor:default` для не-навигируемых узлов + опц. tooltip; не оставлять pointer на неинтерактивном.

### I-08 — Удалить исходную сущность узла: открыть граф (isOrphan)

- **Вердикт:** ✅ OK.
- **Проверено:** `is_orphan` вычисляется server-side НА READ-TIME. `graph.rs:308-328 resolve_node_label`: для `kind==EntityRef` зовёт `resolve_entity_label` (`graph.rs:290-300`, `SELECT title FROM {goal|idea|insight|task} WHERE id=?`); `Some(live)` → обновить label, `is_orphan=false`; `None` (строка-источник удалена) → сохранить STORED label + `is_orphan=true`. Применяется в `list_project_graph` и к `nodes`, и к `external_nodes` (`graph.rs:670, 698`). Свежечитанная строка стартует `is_orphan=false` (`graph.rs:156-159`). Рендер: `EntityRefNode` (`GraphCanvas.tsx:232-241`) → `data.isOrphan ? "источник удалён" : data.label` (`:237`). Тест `'an orphaned entityRef node renders «источник удалён» instead of its stale stored label'` (nodeRenderers.test.tsx:61) — зелёный. Soft-ref survival (D3) покрыт Rust-тестами (delete источника не рушит узел).
- **Обработка ошибок:** н/д (чистое чтение; резолвер тотальный — `None` = orphan-сигнал, не ошибка).
- **Что видит пользователь:** после удаления исходной сущности, при открытии/refresh графа узел показывает «источник удалён» + красную рамку (`nodeCardStyle` `data.isOrphan → statusExited` border, `:196`).
- **Когда именно считается:** каждый `GraphListProject` (mount, push, reconnect). Не кэшируется — всегда re-resolve.
- **Действие:** ничего.

### I-09 — Self-loop / дубль ребра: протянуть (Invariant / Conflict → toast)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (Тексты честны, но сырые en+uuid.)
- **Проверено:** `onConnect` → `orchdGraphAddEdge` → `socket_server.rs:1307-1337` → `graph.rs:508-543 add_edge`:
  - Self-loop: `source==target` → `Invariant("graph edge source and target must differ (no self-loops)")` (`graph.rs:515-519`).
  - Дубль `(source,target,kind)`: unique index `graph_edge_uniq` → `map_edge_conflict` → `Conflict("edge {source}->{target} ({kind}) already exists")` (`graph.rs:256-270`).
  Rust-тесты в `graph::tests` (self-loop→Invariant, dup→Conflict) — зелёные.
- **Точные user-visible тексты (toast):** через `describeOrchdError` (`orchd.ts:769-772`):
  - self-loop → **«недопустимая операция: graph edge source and target must differ (no self-loops)»**
  - дубль → **«конфликт: edge {source-uuid}->{target-uuid} (relates) already exists»**
  Русский префикс + сырой английский backend-message с голыми UUID и `relates`. Читаемо для инженера, не для владельца (O-2 ru/en mixing; те же корни, что F-5/BL-61).
- **Достижимо из UI:** да — drag из source-handle (низ, `Position.Bottom`) в target-handle (верх) того же узла даёт `source==target` → self-loop; повтор существующего ребра → dup. xyflow handles не запрещают.
- **Обработка ошибок:** честная (toast), НО отклонённое ребро остаётся фантомом на канве (optimistic add из I-02 не откатывается) — двойной минус: невнятный текст + висящее ребро.
- **Логи:** FE toast; add_edge — B-04.
- **Что видит пользователь:** тянет self-loop/дубль → toast с англ.-мессиджем + (визуально) ребро осталось до ремоунта/refresh.
- **Дельта от ожидания:** каталог «`Invariant`/`Conflict` → toast; Тексты» — тексты присутствуют, но mixed ru/en с UUID; + фантом (I-02).
- **Действие:** BL (Minor): локализовать/причесать backend-message (или маппить в describeOrchdError по коду в человекочитаемый ru — «нельзя связать узел с самим собой» / «такая связь уже есть»); + откат фантома (I-02).

## Волна 4 — эпики D, K (2026-07-16)

# Эпик D — Цели (и метрики): инвестигейт D-01..D-07

> READ-ONLY инвестигейт по каталогу `docs/qa/ux-first-session-scenarios.md` §2 Эпик D.
> Модель: opus. Все пути прослежены UI-контрол → ipc → wire (proto) → dispatch → persistence.
> Тесты: `cargo test -p bpa-orchd --lib goal` (29 passed), `npx vitest run GoalTree/HomeGoals/FormInsightDialog/orchd` (132 passed).

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| D-01 | ✅ OK | — | Strategic-корень автосоздаётся в `create_project` («Стратегическая цель»), пиннится первым (sort + `list_goals` recursive sort-key), НЕ удаляется/НЕ двигается — ни кнопок в UI, ни на сервере (двойной Invariant + move/delete-guard); title/status правятся осознанно |
| D-02 | 🟡 UX-GAP | Minor | «+ подцель» создаёт `additional` под кликнутым рядом, title «новая цель», refresh + error→toast + orchdDown-гейт — ВСЁ ок, НО нет autoFocus на новый инпут (в отличие от FileTree, с которым doc-comment заявляет паритет) → «немедленной правки» по факту нет, ряд просто появляется внизу без выделения/скролла |
| D-03 | ✅ OK | — | **Эталон реверта.** `commit()` при `!ok` делает `setTitle(goal.title)` (revert к серверному); blank→тихий revert; status честен через controlled-read; `useEffect([goal.title])` синкает внешние апдейты — прямой контрпример к P-27 (IdeasList не ревертит) |
| D-04 | ✅ OK | — | Status честен (controlled), ▲/▼ edge-disabled (`canMoveUp/canMoveDown`), true-swap двумя `move_goal` (ords остаются уникальны). UI-reorder НИКОГДА не меняет `parentId` → cycle-guard (`ancestor_chain_contains`) из UI недостижим → защитный, OK |
| D-05 | 🟡 UX-GAP | Minor | Confirm `«удалить ветку целиком?»` честно говорит про subtree-delete, но НЕ называет число потомков — в отличие от H-06/TasksList `«удалит N подзадач»` (рекурсивный `countDescendants`). Каскад через FK `ON DELETE CASCADE` + `foreign_keys=ON`; error→toast; orchdDown-гейт |
| D-06 | ✅ OK | — | Вторая strategic / strategic-with-parent из UI недостижимы: «+ подцель» хардкодит `"additional"`, «add top-level» аффорданса нет. Серверные Invariant’ы (`project already has a strategic goal` + UNIQUE index `goal_one_strategic_per_project`; `strategic must be a root`) — чисто защитные |
| D-07 | 🟡 UX-GAP | Minor | **O-4: редактора `metric_refs` НЕТ нигде.** `orchdCreateGoal` их не принимает (цели рождаются `'[]'`); `orchdUpdateGoal` принимает `metricRefs`, но ЕДИНСТВЕННЫЕ вызовы (GoalTree) шлют `null`. Бэкенд поддерживает end-to-end (proto/persistence/socket + зелёный round-trip тест), но фронт их не заполняет никогда → единственный потребитель (FormInsightDialog fit-context, `g.metricRefs`) на практике всегда рендерит пусто (ветка `— метрики:` мертва) |

**Итог по эпику:** 4×✅ OK · 3×🟡 UX-GAP (все Minor). Багов нет; доки/спека нигде не переобещали (metrics — открытый вопрос O-4, а не overclaim). Бэкенд целей — крепкий: 29 доменных инвариантов покрыты зелёными тестами.

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| P-27 (несогласованность реверта title/body) | ✅ GoalTree — ЭТАЛОН (ревертит) | D-03; контраст с IdeasList (E-05) |
| O-4 (где владелец правит metric_refs) | 🟡 подтверждён: UI нет | D-07 |
| B-04 (пер-верб tracing нет) | ✅ приемлемо (системно) | D-01..D-07 (goal-вербы без tracing, класс B-04) |

---

## D-01 — Новый проект → «Цели»: strategic-корень уже есть, пиннится, не удаляется/не двигается

- **Вердикт:** ✅ OK.
- **Проверено:** автосоздание — `persistence.rs:1306-1313` (`create_project` в ОДНОЙ tx вставляет strategic `goal`, `STRATEGIC_GOAL_TITLE = "Стратегическая цель"` persistence.rs:604, `metric_refs='[]'`, `parent_id NULL`, `ord 0`) + `crate::graph::seed_strategic_entity_ref` (S4 §5 D6). Тест `create_project_creates_strategic_goal_and_ruleset_row` → **ok** (уже проверялось в C-01). Рендер-пиннинг — `GoalTree.tsx:36-61 buildRows` (сорт: `a.kind==="strategic"?-1:1` затем `ord`, DFS от `parentId=null`) + серверный `list_goals` `WITH RECURSIVE` zero-padded sort-key (`persistence.rs:1729-1742`) гарантирует «единственный `parent_id IS NULL` всегда первым». Non-deletable/non-reorderable в UI: `!isStrategic &&` перед ▲/▼ (GoalTree:231, 243) и «Удалить» (263) — для strategic этих кнопок НЕТ вовсе. Серверные дубль-гарды: `delete_goal` strategic→`Invariant` (persistence.rs:1693-1697, тест `delete_goal_strategic_is_invariant`), `move_goal` strategic→`Invariant` (1640-1644, тест `move_goal_strategic_root_is_invariant`).
- **Обработка ошибок:** есть, честная. Первый маунт с пустым кэшем → `refreshGoals(projectId)` (GoalTree:308-314); rejection у `refreshGoals` сам сурфейсит toast (store, spec §7). Мутаций на этом сценарии нет.
- **Логи:** пер-верб tracing на goal-вербах нет (класс B-04, системное решение прошлых волн). Секретов нет.
- **Что видит пользователь:** открыв «Цели» сразу после создания проекта — корневую строку «Стратегическая цель» (`role="treeitem"`, depth 0) с редактируемым title-инпутом и селектом статуса, но БЕЗ ▲/▼ и БЕЗ «Удалить». `+ подцель` присутствует и на корне. Title/status strategic-корня редактируются осознанно (владелец переименовывает дефолт — doc-коммент create_project «the owner edits it»); это НЕ противоречит «non-deletable/non-reorderable».
- **Дельта от ожидания:** нет. Каталог D-01 «Стратегическая цель-корень уже есть» — соблюдено; автосоздание происходит внутри `create_project`, не отдельным шагом.
- **Действие:** ничего.

## D-02 — «Цели» → «+ подцель»: ряд «новая цель» появился

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `GoalTree.tsx:255-262` (кнопка «+ подцель», `onClick={()=>onAddSubgoal(goal.id)}`) → `handleAddSubgoal(parentId)` (336-343) → `orchdCreateGoal(projectId, parentId, "additional", NEW_SUBGOAL_TITLE="новая цель", "")` → `await refreshGoals(projectId)`. Wire: proto `CreateGoal{project_id,parent_id,kind,title,body}` (lib.rs:782-788) → `socket_server.rs:923-934` → `persistence.rs:create_goal`: `ord = MAX(ord)+1` в группе сиблингов (1532-1536, тест `create_goal_ord_increments_per_sibling_group`) → новый ряд ВНИЗУ детей кликнутого родителя. Тест `GoalTree.test.tsx:97` («+ подцель» зовёт `orchdCreateGoal` с id ряда как parentId + kind «additional» + refresh) → **passed**.
- **Обработка ошибок:** есть, честная. try/catch → `showToast(describeOrchdError(e))` (340-341). orchdDown-гейт: кнопка `disabled={disabled}` где `disabled=orchdDown` (257, 414; тест «while orchdDown … clicking never calls the wrapper» passed).
- **Логи:** пер-верб tracing нет (класс B-04). Секретов нет. Push `GoalsChanged{project_id}` на успехе (`respond_goal`, socket_server.rs:515-527) — плюс явный `refreshGoals` для немедленного обновления ФОРМЫ дерева.
- **Что видит пользователь:** новый ряд «новая цель» (status «активна») появляется как последний ребёнок кликнутого родителя; отступ +1 уровень.
- **Дельта от ожидания:** **нет autoFocus.** Doc-коммент `NEW_SUBGOAL_TITLE` (GoalTree:20-22) заявляет «the owner renames it inline immediately after, same UX as FileTree's inline-rename-after-create», но FileTree реально ставит `autoFocus` на инпут переименования (`FileTree.tsx:543`), а GoalTree — НЕТ (в компоненте ноль `autoFocus`/`.focus()`/`useRef`). То есть «немедленная правка» по факту не наступает: инпут редактируем, но курсор в него сам не встаёт, выделения/скролла к новому ряду тоже нет — при длинном дереве ряд может уехать за пределы вьюпорта. Каталожное «Ряд появился» выполнено; заявленный самим кодом паритет с FileTree — не выполнен.
- **Действие:** BL (Minor): при создании подцели ставить autoFocus (+select) на её title-инпут и/или скроллить к ней — закрыть заявленный паритет с FileTree.

## D-03 — «Цели» → править title (blur/Enter): сохранено через push; отказ → toast + РЕВЕРТ

- **Вердикт:** ✅ OK. **Эталон реверта (референс против P-27).**
- **Проверено:** `GoalRow` держит только in-flight `title` как локальный стейт (GoalTree:180). `commit()` (189-198): trimmed blank → `setTitle(goal.title)` тихий revert (192, никогда не сохраняет пустой title); `trimmed===goal.title` → no-op (195); иначе `const ok = await onTitleCommit(...)`; **`if(!ok) setTitle(goal.title)`** (197) — реверт к серверному значению при отказе. `handleTitleCommit` (318-326) → `orchdUpdateGoal(id, title, null, null, null)`; try→`return true`, catch→`showToast(describeOrchdError(e))`+`return false`. Enter → `blur()` → `onBlur` → `commit()` (208-214). Внешний апдейт не затирается драфтом: `useEffect(()=>setTitle(goal.title),[goal.title])` (185-187).
- **Обработка ошибок:** есть, честная, С РЕВЕРТОМ. Это тот самый GOOD-паттерн, которого нет в `IdeasList.tsx` (P-27, E-05): там локальный `title`/`body` остаётся = отредактированному и НЕ самозалечивается. Здесь при отказе экран немедленно возвращает серверное значение — никогда не врёт о сохранённом.
- **Логи:** пер-верб tracing нет (класс B-04). Секретов нет. Title-правка НЕ делает явный `refreshGoals` (осознанно, doc-коммент 286-288: поля доводит до консистентности общий пайп инвалидации `orchd://goals-changed`, форму дерева — только структурные мутации).
- **Что видит пользователь:** правка сохраняется молча (без toast на успехе); при отказе — toast с честной причиной + инпут откатывается к серверному тексту.
- **Дельта от ожидания:** нет. Каталог D-03 «Отказ → toast + **реверт к серверному значению**» — соблюдено дословно.
- **Действие:** ничего. (Заметка по покрытию: сам revert не покрыт отдельным юнит-тестом — ближайший `an Invariant error … surfaces via showToast`, GoalTree.test:192, проверяет toast, но не откат значения; код при этом однозначен.)

## D-04 — «Цели» → смена статуса; ▲/▼: обновилось; reorder на краю disabled; MoveGoal-циклы недостижимы

- **Вердикт:** ✅ OK.
- **Проверено (статус):** `<select value={goal.status} disabled={disabled} onChange=…>` (GoalTree:217-230) — controlled-read стора. `handleStatusChange` (328-334) → `orchdUpdateGoal(id, null, null, status, null)`; catch→toast. Реверт не нужен: селект отражает `goal.status` из стора, при отказе стор не менялся → селект «сам» показывает старое значение (честно).
- **Проверено (reorder edge-disabled):** `canMoveUp={!isStrategic && idx>0}`, `canMoveDown={!isStrategic && idx>=0 && idx<siblings.length-1}` (412-413); в `GoalRow` — `disabled={disabled || !canMoveUp}` (236) / `!canMoveDown` (248) + `opacity 0.35`. Тест `edge: ▲ on FIRST … ▼ on LAST are disabled and never call orchdMoveGoal` (GoalTree.test:175) → **passed**. True-swap: `swapWithNeighbor` (364-374) захватывает ОБА `ord` и шлёт ДВА `orchdMoveGoal` (каждый берёт чужой старый ord) — иначе (нет `UNIQUE(parent_id,ord)`) односторонний move оставил бы дубль-ord без tiebreaker (doc-коммент 355-362). Тесты `▲/▼ TRUE-SWAPS via TWO orchdMoveGoal` (140, 161) → **passed**.
- **Проверено (cycle-guard недостижим из UI):** `move_goal` server-side имеет `ancestor_chain_contains(&tx, new_parent, id)` → `Invariant "cannot move a goal under itself or one of its own descendants"` (persistence.rs:1664-1668, тест `move_goal_under_own_descendant_or_self_is_cycle_invariant`). НО единственный UI-путь к `move_goal` — ▲/▼, а `swapWithNeighbor` шлёт `orchdMoveGoal(goal.id, goal.parentId, …)` — **`parentId` НЕ меняется** (передаётся `goal.parentId`), меняется только `ord` среди сиблингов. Смена родителя из UI невозможна (нет drag-reparent, нет parent-селекта). ⇒ cycle-guard из UI недостижим → чисто защитный сервер-инвариант → OK.
- **Обработка ошибок:** есть, честная. Каждая мутация в try/catch → toast; orchdDown → все контролы disabled.
- **Логи:** пер-верб tracing нет (класс B-04). Секретов нет. `refreshGoals` один раз после обоих move (структурная мутация).
- **Что видит пользователь:** статус меняется; ▲/▼ переставляет соседей; на краю кнопки серые (0.35) и disabled.
- **Дельта от ожидания:** нет. «MoveGoal-циклы недостижимы из UI» — подтверждено (ответ на каталожный вопрос: да, недостижимы).
- **Действие:** ничего.

## D-05 — «Цели» → удалить ветку: window.confirm → subtree удалён; отказ → toast

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** `handleDelete` (GoalTree:345-353): `if(!window.confirm(DELETE_CONFIRM_TEXT)) return;` где `DELETE_CONFIRM_TEXT = "удалить ветку целиком?"` (18) → `orchdDeleteGoal(id)` → `await refreshGoals(projectId)`; catch→`showToast(describeOrchdError(e))`. Wire: proto `DeleteGoal{id}` «cascades subtree; deleting the strategic root ⇒ Invariant» (lib.rs:802-805) → `socket_server.rs:966-978` (успех → push `GoalsChanged{project_id}`) → `persistence.rs:delete_goal` (1681-1702): strategic→`Invariant`, иначе FK `goal.parent_id REFERENCES goal(id) ON DELETE CASCADE` сносит весь subtree. `foreign_keys=ON` на ОБОИХ путях (persistence.rs:154 in-memory, 202 on-disk; тест `foreign_keys_are_enforced`). Тесты `delete_goal_cascades_subtree`, `delete_goal_strategic_is_invariant` → **ok**. UI: тест `delete asks for confirmation and only calls orchdDeleteGoal after accepted` (GoalTree.test:119, проверяет точный текст `"удалить ветку целиком?"`) → **passed**.
- **Обработка ошибок:** есть, честная. confirm→cancel = no-op; отказ мутации → toast; orchdDown → «Удалить» disabled.
- **Логи:** пер-верб tracing нет (класс B-04). Секретов нет.
- **Что видит пользователь:** нативный `window.confirm("удалить ветку целиком?")`; после OK — вся ветка (цель + все потомки) исчезает.
- **Дельта от ожидания:** confirm НЕ называет число потомков. Каталог D-05 «Проверить: Текст confirm внятен (счёт детей?)» — ответ: счёта нет. Контраст с **H-06**: `TasksList.tsx:51-60` `countDescendants` (рекурсивный) + `deleteConfirmText = "удалить задачу? удалит N подзадач"` (54), при 0 детей строка без «подзадач» (тест TasksList.test:156/192). Здесь текст статичен: владелец не видит масштаба удаления (1 цель или 20 — одинаковый текст). Честно про «целиком», но не квантифицировано → несогласованность с эталоном H-06.
- **Действие:** BL (Minor): привести goal-delete confirm к паттерну H-06 — рекурсивный счёт потомков в тексте («удалит N подцелей»), как в TasksList.

## D-06 — Верб напрямую: вторая strategic / strategic-with-parent → Invariant; достижимо ли из UI

- **Вердикт:** ✅ OK (сервер-инвариант защитный, из UI недостижим).
- **Проверено (UI недостижимость):** единственный create-путь — «+ подцель» → `orchdCreateGoal(projectId, parentId, "additional", …)` (GoalTree:338) — `kind` ХАРДКОД `"additional"`, `parentId` = id существующего ряда. «Add top-level goal» аффорданса нет вовсе (doc-коммент 280-282: «every additional goal always has a parent, so there is no add-top-level affordance»). `orchdCreateGoal`-обёртка (orchd.ts:120-128) принимает `kind`, но в src нет ни одного вызова с `"strategic"` (grep `orchdCreateGoal` → единственный не-тестовый вызов GoalTree:338). ⇒ из UI ни вторую strategic, ни strategic-with-parent создать нельзя.
- **Проверено (сервер-гарды):** `create_goal` (persistence.rs:1489-1530): `(Strategic, Some(parent))`→`Invariant "strategic goal must be a root"` (1495-1499, тест `create_goal_strategic_with_parent_is_invariant`); повторная strategic → `COUNT(*) WHERE kind='strategic' > 0`→`Invariant "project already has a strategic goal"` (1519-1530, тест `create_goal_second_strategic_is_invariant`) + жёсткий UNIQUE index `goal_one_strategic_per_project ON goal(project_id) WHERE kind='strategic'` (schema 279). Также `(Additional, None)`→`Invariant "additional goal requires a parent"` (тест `create_goal_additional_without_parent_is_invariant`) и `move_goal` additional→root→`Invariant` (тест `move_goal_additional_to_root_is_invariant`). Все зелёные.
- **Обработка ошибок:** есть (сервер), честная. Если бы верб дёрнули напрямую — `describeOrchdError`→«недопустимая операция: …» (англ. Invariant-хвост, класс O-2, но путь из UI недостижим).
- **Логи:** пер-верб tracing нет (класс B-04). Секретов нет.
- **Что видит пользователь:** из UI — ничего (сценарий не воспроизводим); инвариант охраняет только прямой верб/восстановление БД.
- **Дельта от ожидания:** нет. Каталог D-06 «Достижимо ли из UI вообще (если нет — ок)» — ответ: недостижимо → OK.
- **Действие:** ничего.

## D-07 — «Цели» → найти редактор metric_refs (O-4)

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.** (O-4 подтверждён: UI-редактора metric_refs НЕТ.)
- **Проверено (нет сеттера в UI):** grep всего `src/` по `metric_refs|metricRefs` (не-тест): только `orchd.ts` (обёртка), `orchd-types.ts` (тип `Goal.metricRefs: Array<string>`), `FormInsightDialog.tsx` (потребитель). `orchdCreateGoal` (orchd.ts:120-128) НЕ принимает metric_refs вовсе — цели рождаются `metric_refs='[]'` (persistence.rs:1544, а strategic — 1311). `orchdUpdateGoal` (130-138) ПРИНИМАЕТ `metricRefs: string[] | null`, но ЕДИНСТВЕННЫЕ не-тестовые вызовы — `GoalTree.tsx:320` (`orchdUpdateGoal(id, title, null, null, **null**)`) и `:330` (`orchdUpdateGoal(id, null, null, status, **null**)`) — оба шлют `null`. Ни одного UI-контрола (инпут/чипы/модалка) для metric_refs в дереве целей нет.
- **Проверено (бэкенд полностью готов, но фронт его не кормит):** proto `UpdateGoal{…, metric_refs: Option<Vec<String>>}` (lib.rs:789-794); `socket_server.rs:936-953` прокидывает `metric_refs.as_deref()` в `db.update_goal`; `persistence.rs:update_goal` (1566-1608) пишет `metric_refs = COALESCE(?5, metric_refs)` — единственный write-path; тест `update_goal_updates_fields_and_metric_refs_round_trip` (persistence.rs:3398, `["m1","m2"]` round-trip) → **ok**. Т.е. цепочка wire→persistence умеет metric_refs, но фронт никогда не передаёт non-null.
- **Проверено (потребитель на практике всегда пустой):** единственный рендер — `FormInsightDialog.tsx:414` `{g.metricRefs.length > 0 && ` — метрики: ${g.metricRefs.join(", ")}`}` в fit-context «Контекст для оценки» → «Цели проекта». Поскольку metric_refs никогда не заполняются из UI (create — без них, update — всегда null), в реальном приложении `g.metricRefs` всегда `[]` → ветка `— метрики:` **мёртвый код на практике**. Тест `fit-context: fetches and renders the project's goals with metric_refs` (FormInsightDialog.test:163) зелёный, но использует ФИКСТУРУ `metricRefs: ["mrr"]` — состояние, которое приложение своими средствами породить не может.
- **Обработка ошибок / логи / гейт:** N/A — контрола нет.
- **Что видит пользователь:** в дереве целей — никакого способа задать/увидеть метрики цели. В fit-context при форминге инсайта «Цели проекта» перечисляет только заголовки целей; строка «— метрики: …» не появляется никогда (реальные metric_refs пусты).
- **Дельта от ожидания (адъюдикация O-4):** metric_refs спроектированы сквозняком (схема `metric_refs TEXT NOT NULL DEFAULT '[]'` Q12-forward, тип, wire, persistence, update-обёртка, потребитель fit-context), НО owner-facing СЕТТЕР отсутствует целиком → capability инертна, а fit-context-фича с метриками — недостижима в проде. Классификация: **🟡 UX-GAP (Minor)** — ничего не ломается, просто заявленный fit-context-контекст (цели+**метрики**+граф) по метрикам всегда пуст. Правдоподобно — осознанная отсрочка на будущий слайс (тогда ближе к 📄), но по факту v0.7.0 это дыра, требующая решения владельца (O-4: где владелец правит metric_refs?).
- **Действие:** отметить O-4 как подтверждённый (UI нет). Решение владельца: либо (a) добавить редактор metric_refs в GoalRow (инпут/чипы → `orchdUpdateGoal(id,null,null,null,[...])` — обёртка и бэкенд уже готовы, нужен только UI), либо (b) явно пометить metric_refs как отложенную фичу и убрать/задизейблить metrics-ветку в fit-context, чтобы не создавать впечатление рабочего контекста. До решения — 🟡 UX-GAP.

---

## Что НЕ удалось проверить (и почему)

- **Реальный in-app рендер** (визуальный autoFocus/скролл D-02, живой каскад D-05, живой toast) — среда READ-ONLY, без запуска Tauri-приложения; выводы построены на статике кода + зелёных юнит/persistence-тестах.
- **Прямой юнит-тест реверта title (D-03)** в репозитории отсутствует (revert-строка `GoalTree.tsx:197` покрыта косвенно через error→toast тест, но не проверяет откат значения) — код однозначен, но точечного assert’а на revert нет.
- **Живой путь заполнения metric_refs (D-07)** воспроизвести нечем: приложение не имеет средства породить non-empty metric_refs; проверено только статически, что единственный write-path (update_goal) из UI зовётся исключительно с `null`.

# Эпик K — Кросс-каттинг: инвестигейт K-01..K-07

> READ-ONLY инвестигейт по каталогу `docs/qa/ux-first-session-scenarios.md` §2 Эпик K.
> Модель: opus. Пути прослежены UI-контрол → store → ipc → verb → dispatch → persistence.
> Ничего в репозитории не менялось. launchd/реальные процессы не трогались.

## Сводная таблица вердиктов

| ID | Вердикт | Severity | Суть одной строкой |
|---|---|---|---|
| K-01 | 🟡 UX-GAP | Important | Toast — очередь-из-ОДНОГО (`showToast` затирает мгновенно), автозакрытие 4с, ручного закрытия НЕТ (`dismissToast` определён+тестируется, но не подключён ни к одному UI-контролу — мёртв). Под пачкой ошибок сообщения провабельно теряются (N+1 в IdeasList, mutation-toast → refresh-error-toast). |
| K-02 | 🟡 UX-GAP | Important | `onOrchdUp` рефетчит ТОЛЬКО `projects` (+ 6 слайсов открытого проекта, если `activeProjectId!==null`). НЕ рефетчит: MCP servers/tools/artifacts, connectors, skills, invocations, audit, policies, research runs, global-ruleset. F-10 подтверждён и обобщён на всю поверхность «Расширения»+Журнал+глобальные правила. |
| K-03 | ✅ OK | — | Оба баннера есть («файл утерян»+[Создать заново] / «файл изменён снаружи»+[Принять]); `acknowledge_rule_file` — отдельный `Invariant("file missing")` + `Io` для прочих read-ошибок. (Минор: англ. «file missing» протекает в toast — O-2; concurrent in-app upsert-clobber не детектится — см. K-07.) |
| K-04 | 🟡 UX-GAP | Minor | Ноль лимитов длины/валидации пустоты/нормализации — ни на клиенте (нет `maxLength`), ни на сервере (`title`/`body` пишутся в SQLite verbatim). Единственная граница — 16 MiB wire-frame (`MAX_FRAME_LEN`). 100КБ-тело принимается (~0.6% капа) и рендерится целиком в plain-map списках/сайдбаре/пуш-payload (связка с K-06). Честно (без молчаливого обрезания), но без guardrails. |
| K-05 | ✅ OK | Minor | Single-instance на уровне приложения НЕ энфорсится (нет `tauri-plugin-single-instance`). НО мультиклиент поддержан by design: orchd accept-loop даёт каждому коннекту уникальный `conn_id` и регистрирует в общий `Broadcaster`, пуши фанятся всем; boot-kickstart non-force/идемпотентен. Два экземпляра → два клиента к одному демону → LWW+fan-out сводят. Undefined-но-безвредно. |
| K-06 | 🟡 UX-GAP | Minor | Виртуализирован только FileTree (собственный windowing). IdeasList/TasksList/InsightsList/HomeGoals/GoalTree/ArtifactsTab/InvocationLog — plain `.map()` (нет react-window/Virtuoso в deps). GraphCanvas: `<ReactFlow>` без `onlyRenderVisibleElements` → все узлы в DOM. Реальный N+1: IdeasList на маунте фаерит `refreshResearchRuns` НА КАЖДУЮ идею. |
| K-07 | 🟡 UX-GAP | Minor | Чистый last-write-wins везде (ноль optimistic-concurrency: нет version/etag/updated_at-guard). Пуши сводят оба окна честно. Read-modify-write clobber: rank/ord (абсолютный midpoint из stale-списка — H-04) и **upsert_ruleset — whole-document** (md+policy заменяются целиком, md_hash обновляется → второе окно НЕ увидит ExternallyModified). Конвергенция честная, но конфликт-детекции ноль. |

**Итог по эпику:** 2×✅ OK (K-03, K-05) · 5×🟡 UX-GAP (K-01/K-02 Important + K-04/K-06/K-07 Minor) · 0×🔴 · 0×📄.

## Реестр подозрений (вердикты)

| Подозрение | Вердикт | Где подтверждено |
|---|---|---|
| P-21 (Toast: один слот, авто-4с, dismissToast мёртв) | 🟡 ПОДТВЕРЖДЁН | K-01 |
| F-10 (onOrchdUp не рефетчит research runs) | 🟡 ПОДТВЕРЖДЁН и ОБОБЩЁН (весь ext+audit+global-ruleset) | K-02 |
| H-04 (rank midpoint из stale — clobber) | 🟡 подтверждён (+ goal ord, + ruleset whole-doc) | K-07 |
| P-19 (двойной сабмит) — смежно N+1 | 🟡 см. K-06 (N+1 усиливает K-01 clobber) | K-06 |
| K-04 отсутствие валидации длины | 🟡 подтверждён статически | K-04 |

---

## K-01 — Две ошибки подряд: второй toast затирает первый через <4с

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.**
- **Проверено:** `src/store/store.ts:548-559` (`showToast`), `:561-565` (`dismissToast`), `:379-380` (`TOAST_AUTO_DISMISS_MS = 4000`); `src/components/Toast.tsx:20-48` (рендер); `src/store/store.ts:574-727` (все `refresh*` → `showToast(describeOrchdError(e))`); `src/components/IdeasList.tsx:390-396` (N+1-loop). Использование `dismissToast`: только `store.test.ts`/`Toast.test.tsx` — в проде **ноль** вызовов (`grep -rn dismissToast src/`).
- **Обработка ошибок:** честная, но queue-of-ONE. `showToast` (549-551): `clearToastTimer()` → `set({ toast: message })` — безусловно ЗАМЕНЯЕТ текущий toast. Токен-гвард (550, 556) защищает только от того, чтобы старый таймер не гасил НОВЫЙ toast — он НЕ сохраняет старое сообщение. `<Toast/>` — чистый ридер `s.toast`, без кнопки закрытия/onClick (Toast.tsx:24-47: `<div role="alert">{toast}</div>`, никакого «×»).
- **Логи:** UI-слой не логирует (toast — единственный канал). Сообщение = `describeOrchdError` (человекочитаемое, без секретов); `Invariant`→«недопустимая операция: <msg>», `disconnected`→«оркестратор недоступен» (`orchd.test.ts:817-876`).
- **Что видит пользователь:** ровно ОДНО сообщение за раз, ≤4с, гасится само, вернуть/закрыть вручную нельзя. Провабельные потери (grounded в коде):
  1. **N+1 в IdeasList при сбое orchd** (`IdeasList.tsx:392-393`): на маунте для каждой из N идей `refreshResearchRuns(idea.id)`; при `orchdDown`/сбое КАЖДЫЙ отказ зовёт `showToast` в тесном цикле → видно только ПОСЛЕДНЕЕ из N, первые N-1 затёрты за микросекунды.
  2. **mutation-toast → refresh-error-toast**: успешная мутация показала «…создано», следом её `refresh*` (или прилетевший `*-changed`-пуш) падает → error-toast мгновенно накрывает success.
  3. **Два доменных пуша при флапе orchd**: `onOrchdIdeasChanged`→`refreshIdeas` (fail→toast) и `onOrchdTasksChanged`→`refreshTasks` (fail→toast) — второй затирает первый.
- **Дельта от ожидания:** каталог ждёт «оба сообщения прочитываемы» — не выполняется. Дизайн осознанно выбрал queue-of-one (`store.ts:125-131` doc: «at most one thing asks for attention»), но (а) `dismissToast` — мёртвый код (нет UI-триггера) и (б) под bursty-сбоями сообщения теряются без истории.
- **Действие:** BL-кандидат: либо мини-очередь/стек toast-ов + видимая кнопка закрытия (подключить существующий `dismissToast`), либо хотя бы «(+N ещё)»-счётчик и ручное закрытие. Important, т.к. это системный «honest error surface», а под ошибками он молчит про всё, кроме последнего.

## K-02 — orchd восстановился (`orchd://up`): полная регидрация открытых табов

- **Вердикт:** 🟡 UX-GAP. **Severity: Important.**
- **Проверено:** `src/App.tsx:248-276` (`onOrchdUp`-хендлер, прочитан полностью). Доступные `refresh*` в сторе: `store.ts:574-727` (16 действий). Маунт-фетчи ext-табов: `ServersTab`/`ConnectorsTab.tsx:250-251`/`SkillsTab.tsx:161-162`/`InvocationLog.tsx:160`/`ArtifactsTab.tsx:163-164`/`ToolsBrowser.tsx:138-140` — все на `useEffect` (один раз при маунте), далее полагаются на пуши.
- **Что `onOrchdUp` РЕФЕТЧИТ:** `s.setOrchdDown(false)`; `s.refreshProjects()` — ВСЕГДА; и ТОЛЬКО если `activeProjectId!==null` (т.е. открыт ProjectPanel): `refreshGoals`, `refreshTasks`, `refreshIdeas`, `refreshInsights`, `refreshRuleset('project:'+id)`, `refreshGraph` (App.tsx:262-274).
- **Что `onOrchdUp` ПРОПУСКАЕТ** (проверено grep'ом хендлера — ни одного из этих вызовов внутри 248-276):
  - **research runs** — `refreshResearchRuns` (F-10 — ПОДТВЕРЖДЁН);
  - **MCP** — `refreshMcpServers`, `refreshMcpTools`, `refreshMcpArtifacts`;
  - **connectors** — `refreshAccounts`;
  - **skills** — `refreshSkills`;
  - **audit/журнал** — `refreshInvocations`, `refreshAuditRows`, `refreshPolicies`;
  - **global-scope ruleset** — `refreshRuleset('global')` (рефетчится только `project:<id>`, и только при открытом проекте).
- **Обработка ошибок:** каждый `refresh*` честно тостит `describeOrchdError` при отказе (`store.ts`), но это про сам вызов; проблема — что вызовы НЕ делаются.
- **Что видит пользователь:** если во время бунса orchd открыт таб «Расширения» (Серверы/Инструменты/Коннекторы/Навыки/Журнал/Артефакты) или ResearchPane — их слайсы НЕ обновляются на реконнекте. Маунт-эффект таба не перезапускается (компонент не ремоунтится), а `onOrchdUp` их не трогает → данные остаются как были ДО бунса. Худший случай (как F-10): на холодном старте маунт-фетч таба проиграл гонку с ~4с bring-up orchd, слайс пуст, тост «оркестратор недоступен» — и `onOrchdUp` его НЕ долечивает (в отличие от `projects`, у которого self-heal есть, App.tsx:249-262). Восстановление только ручное: уйти с таба и вернуться (ремоунт → маунт-фетч) либо дождаться мутирующего пуша.
- **Дельта от ожидания:** каталог ждёт «полная регидрация открытых табов» — фактически регидрируется только домен проекта. Вся S-EXT-поверхность + research + audit + global-ruleset не покрыты.
- **Действие:** BL-кандидат: расширить `onOrchdUp` до безусловного рефетча всех whole-store слайсов (`refreshMcpServers`/`refreshMcpArtifacts`/`refreshAccounts`/`refreshSkills`/`refreshInvocations`/`refreshAuditRows`/`refreshPolicies` + `refreshRuleset('global')`), а scoped (`refreshMcpTools`/`refreshResearchRuns`) — по уже закэшированным ключам. Это ровно тот же паттерн self-heal, что уже применён к `projects`.

## K-03 — Rules-файл удалён/изменён извне: баннеры + AcknowledgeRuleFile

- **Вердикт:** ✅ OK.
- **Проверено:** классификация — `crates/orchd/src/ruleset_files.rs:67-77` (`read_state`: `Ok`/`ExternallyModified`/`Missing`, прочие read-fail → `Missing`); dispatch — `socket_server.rs:581-644` (`build_ruleset_view`), `:1206-1215` (`AcknowledgeRuleFile`); persistence — `persistence.rs:2611-2633` (`acknowledge_rule_file`); UI — `RulesetPanel.tsx:17-18` (копии), `:427-441` (`externallyModified`→баннер+[Принять]), `:442-456` (`missing`→баннер+[Создать заново]).
- **Обработка ошибок / оба пути:**
  - `externallyModified` (hash не сошёлся): `read_state` возвращает `(Some(content), ExternallyModified)` → `RulesetPanel.tsx:427` рисует `ruleset-banner-modified` + [Принять] (`AcknowledgeRuleFile` → rehash).
  - `missing` (файла нет / нечитаем): `(None, Missing)` → `RulesetPanel.tsx:442` рисует `ruleset-banner-missing` + [Создать заново] (`UpsertRuleSet{mdContent:""}`). Прочие read-ошибки (перм, non-UTF8, директория) честно свёрнуты в `Missing` (ruleset_files.rs:76).
  - `acknowledge_rule_file` — ОТДЕЛЬНЫЙ путь для гонки «баннер сказал modified, но к моменту клика файл удалён»: `persistence.rs:2616-2622` — `NotFound`(io) → `Invariant("file missing")`; прочие io-ошибки → `Io(...)`; неизвестный `id` → `NotFound`; архивный проект → guard до чтения файла (`:2614`). Три различимых исхода, не проглочены.
- **Логи:** файл-контент НИКОГДА не логируется (ruleset_files.rs:60, spec §5 no-secrets).
- **Что видит пользователь:** корректные различимые баннеры и действия; после [Принять]/[Создать заново] `RulesetPanel` рефетчит вью (RulesetPanel.tsx:298-303) → баннер исчезает.
- **Дельта от ожидания:** нет. Два минорных смежных замечания (не дефекты K-03): (1) `Invariant("file missing")` протекает в toast как «недопустимая операция: file missing» — англ. строка в ru-UI (общий O-2). (2) In-app конкурентный `UpsertRuleSet` из второго окна обновляет `md_hash` в БД → первое окно на следующем `GetRuleSet` увидит `Ok`, а НЕ `ExternallyModified`, то есть file-state-механизм ловит внешние правки, но НЕ чужой клиентский upsert (см. K-07).
- **Действие:** ничего по K-03. Языковую правку «file missing» — в общий тикет O-2.

## K-04 — Лимиты ввода: 10к-символьные строки, эмодзи, RTL

- **Вердикт:** 🟡 UX-GAP (robustness). **Severity: Minor.**
- **Проверено:** серверная сторона — `persistence.rs:1761-1781` (`create_idea`: `title`/`body` уходят в `INSERT` params verbatim, ноль проверок), `:1787-1818` (`update_idea` — то же), аналогично `create_goal:1477`/`create_insight:1918`/`create_task:2149`/`create_project:1276`. Единственные `Validation`-ветки в персистентности — про `workspace_ids.is_empty()` (1282), политику (2405-2425), `md_path` (2554-2560): **ни одной про длину/пустоту title/body**. Клиент — `grep -rnE "maxLength" src/components/` → **пусто** (ни на одном input/textarea). Wire-cap — `crates/protocol/src/framing.rs:21` `MAX_FRAME_LEN = 16*1024*1024`; orchd переиспользует тот же codec (`orchd-proto/src/lib.rs:1313-1321` через `bpa_protocol::encode_cbor_frame`/`CborFrameDecoder`).
- **Обработка ошибок:** честная, но нулевые guardrails. Значение любой длины и любого юникода (эмодзи/RTL/комбинирующие) пишется в SQLite как UTF-8 без нормализации — это КОРРЕКТНО (SQLite хранит байты как есть; никакого молчаливого обрезания). Единственный отказ — если ВЕСЬ CBOR-фрейм превысит 16 MiB: `CborFrameDecoder` отвергнет фрейм (framing error → дисконнект запроса), честно, без порчи данных.
- **Что видит пользователь:** 100КБ-тело (~0.6% капа) принимается и сохраняется. Далее оно рендерится ЦЕЛИКОМ: в plain-`.map()` списках (K-06), в сайдбаре, и в payload доменных пушей (весь объект гоняется по сокету на каждое `*-changed`). Нет усечения в превью, нет «…ещё N символов». RTL/эмодзи отрисуются как обычный текст (без нормализации/санитайза — XSS-риска нет, React экранирует).
- **Дельта от ожидания:** каталог ждёт «рендер не ломается, сервер не режет молча» — сервер и правда не режет (честно), но и лимитов нет вообще: патологическое тело деградирует рендер/пуши и раздувает БД без единого предупреждения.
- **Действие:** BL-кандидат (Minor): ввести разумные капы (напр. title ≤512, body ≤64КБ) с клиентским `maxLength` + серверным `Validation` (defense-in-depth), и усечение в списочных превью. Не срочно (нет краха/потери данных), но «production-grade» ожидает guardrails.

## K-05 — Второй экземпляр приложения: single-instance guard

- **Вердикт:** ✅ OK (мультиклиент поддержан). **Severity: Minor** (app-level guard отсутствует, но безвреден). Формулировка «supported/unsupported/undefined»: **мультиклиент — SUPPORTED by design; app single-instance — UNDEFINED/незагвардён, но безвредно.**
- **Проверено:** single-instance — `grep -rn single_instance src-tauri/ Cargo.*` → **пусто**; плагины в `lib.rs:638-642` — только `store`/`dialog`/`fs`/`shell` (нет `tauri-plugin-single-instance`); `Cargo.toml:16-36` — плагин не в зависимостях. Мультиклиент orchd — `socket_server.rs:151-197` (accept-loop: `next_conn_id: u64=1`, инкремент на каждый коннект :185-186, `conns.spawn(handle_client(conn_id,...))`), `:243` (`broadcaster.register(conn_id, out_tx)`), `broadcast.rs:62-67` (`broadcast` фанит `try_send` ВСЕМ зарегистрированным). Boot-kickstart non-force — доказано тестом `lib.rs:1032-1067` (`ensure_daemon_running_uses_non_force_kickstart_on_boot`: kickstart БЕЗ `-k`).
- **Что происходит при двух экземплярах:** оба процесса гонят `bring_up_daemon`/`bring_up_orchd` = install_agent+bootstrap(идемпотентно)+kickstart(non-force, НЕ убивает живой демон/сессии). Оба коннектятся к одним сокетам, получают разные `conn_id`, регистрируются каждый в общий `Broadcaster` → оба получают КАЖДЫЙ пуш. БД `orchd.db` открыта только ДЕМОНОМ (единый launchd-managed на пользователя), не приложением → нет lock-contention. Мутации из обоих окон → LWW в демоне; `*-changed`-пуши сводят оба экрана.
- **Практическая экспозиция:** macOS LaunchServices на обычный двойной клик по .app НЕ поднимает второй экземпляр (активирует существующий). Второй инстанс достижим только через `open -n`/запуск бинаря напрямую. Т.е. редко, и когда случается — архитектура переносит это когерентно.
- **Дельта от ожидания:** каталог ставит «???». Ответ: два окна на один сокет — поддержано (broker мультисабскрайб), не крашится, сводится. Единственный тонкий край: обе сессии могут аттачнуться к одному PTY (broker attach-map, территория sessiond) — вне скоупа K, отмечено как edge.
- **Действие:** ничего обязательного. Опц. Minor: добавить `tauri-plugin-single-instance` с фокусировкой существующего окна — косметика, не корректность.

## K-06 — 100+ идей/задач/узлов: виртуализация и деградация списков

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** виртуализация — `grep -rlnE "react-window|FixedSizeList|Virtuoso|useVirtual" src/` → только упоминаний нет, в `package.json` виртуализирующих либ нет (есть `@xyflow/react`). FileTree — собственный windowing: `FileTree.tsx:653-668` (`visibleCount = ceil(viewportHeight/ROW_HEIGHT)+OVERSCAN*2`, `nodes.slice(startIndex,endIndex)`, `visible.map(renderRow)`). Остальные — plain `.map()`: `IdeasList.tsx:493`, `TasksList.tsx:446`, `InsightsList.tsx:360`, `HomeGoals.tsx:149/167`, `GoalTree.tsx:402`, `ArtifactsTab.tsx:179`, `InvocationLog.tsx:291/332`. GraphCanvas — `GraphCanvas.tsx:522-533`: `<ReactFlow nodes={displayNodes} ...>` БЕЗ `onlyRenderVisibleElements` (дефолт xyflow = false → все узлы в DOM).
- **N+1 refresh (реальный):** `IdeasList.tsx:390-396` — на маунте `for (const idea of rows) if (!(idea.id in researchRunsByIdea)) void refreshResearchRuns(idea.id)`. Каждый вызов = отдельный `research_list_runs(ideaId)` IPC → сокет-round-trip → SQL. 100 идей = до 100 параллельных IPC на первый рендер списка (и до 100 toast-ов при сбое — усиливает K-01). Тест это фиксирует как намеренное: `IdeasList.test.tsx:321` («eagerly fetches research runs ... for every rendered idea, once»). Смежно: `ToolsBrowser.tsx:138-140` — тот же per-server `refreshMcpTools`.
- **Что видит пользователь:** при 100+ элементах списки идей/задач/инсайтов/целей/артефактов/журнала рендерят все ряды в DOM (нет окна) — растёт время маунта/скролл-джанк; xyflow-граф держит все узлы в DOM (canvas-рендер терпимее, но без culling). Первый вход в «Идеи» с большим числом идей = всплеск IPC/DB.
- **Дельта от ожидания:** каталог ждёт «приемлемая отзывчивость». Корректность не страдает — это чистая деградация производительности на масштабе.
- **Действие:** BL-кандидат (Minor): виртуализировать длинные списки (переиспользовать паттерн FileTree или ввести react-window), выставить `onlyRenderVisibleElements` на ReactFlow, и заменить IdeasList N+1 на батч-verb (`research_list_runs` по массиву ideaId одним вызовом).

## K-07 — Правки в двух окнах/гонки: LWW + сведение пушами

- **Вердикт:** 🟡 UX-GAP. **Severity: Minor.**
- **Проверено:** optimistic-concurrency — `grep -rnE "WHERE.*updated_at|version =|expected_version|if_match|etag" persistence.rs` → **пусто**; все `UPDATE ... WHERE id=?1` безусловны. `set_task_rank:2296-2309` (`UPDATE task SET rank=?2 WHERE id=?1` — клиент шлёт абсолютный `rank: f64`). `move_goal:1617-1671` (`UPDATE goal SET parent_id=?2, ord=?3 WHERE id=?1` — абсолютный `ord`). `upsert_ruleset:2543-2597` — whole-document (md_content пишется атомарно `write_atomic`, `policy`+`md_path`+`md_hash` заменяются целиком; **нет параметра expected-hash/version**). Сведение — каждый мутирующий verb бродкастит `*-changed` (broadcast.rs + App.tsx:198-243 → `refresh*`).
- **Конвергенция (честная):** ДА. Любая мутация из любого окна → `*-changed`-пуш обоим клиентам → оба рефетчат → оба консистентны с БД (последней записью). Экраны сходятся.
- **Read-modify-write clobber (по возрастанию остроты):**
  1. **rank / ord** (`set_task_rank`, `move_goal`): клиент считает midpoint из ОТОБРАЖЁННОГО (возможно stale) списка и шлёт абсолютное значение. Два конкурентных реордера из stale-состояния → пересекающиеся rank/ord (видимая аномалия порядка, не порча; H-04 покрыл).
  2. **upsert_ruleset — whole-document (острейший)**: два окна открыли Правила с одной базой; A сохраняет (md_hash→hashA); B сохраняет свой stale-контент (md_hash→hashB) → правки A СТИРАЮТСЯ полностью (заменяются и markdown-тело, и policy). Конфликт НЕ поднимается (нет сравнения ожидаемого hash). Хуже: т.к. upsert обновляет `md_hash` в БД под новый контент, следующее `GetRuleSet` у окна A вернёт `state=Ok` (совпадающий hash), а НЕ `ExternallyModified` → НИКАКОГО баннера-предупреждения о том, что его правки затёрты (K-03-механизм ловит только внешние правки файла, не чужой клиентский upsert).
- **Что видит пользователь:** оба окна в итоге показывают одно и то же (LWW-состояние БД) — сведение честное. Но при одновременном редактировании одной сущности (особенно Правил) проигравший тихо теряет свои правки без конфликт-нотиса.
- **Дельта от ожидания:** каталог ждёт «last-write-wins + пуши сводят + видимые аномалии». LWW и сведение — есть и честные; аномалии — rank/ord-перекос и молчаливый ruleset-clobber.
- **Действие:** BL-кандидат (Minor): для `UpsertRuleSet` завести optimistic-concurrency (передавать `expected_md_hash`; при рассинхроне → `Conflict` вместо тихого затирания, роняя пользователя в тот же [Принять]-flow, что и `ExternallyModified`). rank/ord — приемлемо в v1 (LWW осознан). Конвергенция как таковая честна — не 🔴.

## Что НЕ удалось проверить (и почему)

- **Динамический репро под нагрузкой** (реальные 100+ элементов, реальный конкурентный upsert из двух окон, реальный второй .app-инстанс): требует запуска приложения/демонов и мутации живых процессов — задача READ-ONLY, launchd/процессы не трогались. Все вердикты выведены статически из кода/тестов (пути прослежены до строк).
- **Broker attach-map при двух клиентах на один PTY** (K-05 edge): это территория `bpa-sessiond`/`broker.rs`, вне 7 сценариев K; отмечено как потенциальный edge, но не прослеживалось до конца.
- **Точный лимит рендера большого тела в браузере** (K-04): «ломается ли» 1МБ/10МБ-строка в конкретном списке — зависит от рантайма webview; статически подтверждено лишь отсутствие усечения/капов и полный проброс в пуши.
