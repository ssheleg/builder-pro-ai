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
