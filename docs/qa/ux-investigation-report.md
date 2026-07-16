# UX-инвестигейт — единый отчёт (фаза 2)

> Заполняется циклом глубокой проверки по каталогу `docs/qa/ux-first-session-scenarios.md`.
> Один сценарий = одна секция ниже + отметка в чек-листе. Цикл идёт, пока есть ⬜.

**Вердикты:** ✅ OK — работает, обработки/логи на месте · 🟡 UX-GAP — работает, но юзер
страдает (нет фидбека/индикации/пути восстановления) · 🔴 BUG — дефект поведения ·
📄 DOC-GAP — код ок, доки врут/молчат.

## Чек-лист (статус всех сценариев)

| Эпик | Сценарии | Статус |
|---|---|---|
| A — запуск/демоны | A-01 ⬜ · A-02 ⬜ · A-03 ⬜ · A-04 ⬜ · A-05 ⬜ · A-06 ⬜ · A-07 ⬜ · A-08 ⬜ · A-09 ⬜ · A-10 ⬜ | 0/10 |
| B — workspace/файлы/терминал | B-01 ⬜ · B-02 ⬜ · B-03 ⬜ · B-04 ⬜ · B-05 ⬜ · B-06 ⬜ · B-07 ⬜ · B-08 ⬜ · B-09 ⬜ · B-10 ⬜ · B-11 ⬜ · B-12 ⬜ · B-13 ⬜ · B-14 ⬜ | 0/14 |
| C — проект | C-01 ⬜ · C-02 ⬜ · C-03 ⬜ · C-04 ⬜ · C-05 ⬜ · C-06 ⬜ · C-07 ⬜ · C-08 ⬜ · C-09 ⬜ | 0/9 |
| D — цели/метрики | D-01 ⬜ · D-02 ⬜ · D-03 ⬜ · D-04 ⬜ · D-05 ⬜ · D-06 ⬜ · D-07 ⬜ | 0/7 |
| E — идеи (гипотезы) | E-01 ✅ · E-02 ✅ · E-03 ✅ · E-04 ✅ · E-05 🟡 · E-06 ✅ · E-07 🟡 · E-08 🟡 | 8/8 |
| F — research | F-01 ✅ · F-02 ✅ · F-03 🔴 · F-04 🟡 · F-05 🟡 · F-06 🟡 · F-07 ✅ · F-08 🟡 · F-09 🟡 · F-10 🟡 · F-11 🟡 · F-12 ✅ · F-13 ✅ · F-14 ✅ | 14/14 |
| G — инсайты | G-01 ✅ · G-02 🟡 · G-03 ✅ · G-04 ✅ · G-05 🟡 · G-06 🟡 · G-07 ✅ · G-08 🟡 | 8/8 |
| H — задачи (фичи) | H-01 ⬜ · H-02 ⬜ · H-03 ⬜ · H-04 ⬜ · H-05 ⬜ · H-06 ⬜ · H-07 ⬜ | 0/7 |
| I — граф | I-01 ⬜ · I-02 ⬜ · I-03 ⬜ · I-04 ⬜ · I-05 ⬜ · I-06 ⬜ · I-07 ⬜ · I-08 ⬜ · I-09 ⬜ | 0/9 |
| J — расширения | J-01 ⬜ · J-02 ⬜ · J-03 ⬜ · J-04 ⬜ · J-05 ⬜ · J-06 ⬜ · J-07 ⬜ · J-08 ⬜ | 0/8 |
| K — кросс-каттинг | K-01 ⬜ · K-02 ⬜ · K-03 ⬜ · K-04 ⬜ · K-05 ⬜ · K-06 ⬜ · K-07 ⬜ | 0/7 |
| **Итого** | | **30/101** — 16 ✅ · 13 🟡 · 1 🔴 (F-03/B-01 Critical → BL-89) |

## Реестр вердиктов по подозрениям

| Подозрение | Сценарии | Вердикт | Секция |
|---|---|---|---|
| P-01…P-28 | см. каталог §1 | — | — |
| B-01…B-10 | см. каталог §1 | — | — |
| F-1…F-8 | см. каталог §1 | — | — |

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
