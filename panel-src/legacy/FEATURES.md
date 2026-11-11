# What the old page does (inventory for the rebuild)

Source: `legacy/page.estacion-viva.html` (3,015 lines). `L123` is a line of that file. Field names are those of
`crates/jobbot-panel/src/snapshot.rs` (`snapshot.x.y`); `card` is a `Card`, `detail` a `JobDetail`. The logic is already ported
(`src/core/*`, wording in `src/i18n/*/common.js`); this file is for whoever builds a room: what must still be shown,
what each button does, and which odd behaviours exist on purpose. Spanish quoted below is the old wording.

Layout of the file: head script (theme) L8-16 · CSS L18-620 · HTML shell L622-647 · script L648-3012
(helpers L724-1000, board L953-1307, ticket L1309-1469, maps L1471-1638, header and tabs L1640-1721, Hoy L1760-1885,
Procesos L1887-1994, Cola L1996-2018 and L2168-2298, night strip L2020-2129, drawer L2300-2522, Aprender L2524-2604,
motion and routing L2606-2750, keys L2778-2892, palette L2902-2983, boot L3002-3011).

## 1. Server contract

| Call | Body or query | Answer | Used for |
| --- | --- | --- | --- |
| GET `/api/snapshot` | none | `Snapshot` | everything (poll every 30 s, L3011) |
| GET `/api/job?key=` | key | `JobDetail`, or 404 `{message}` | drawer (L2340) |
| GET `/api/shot?key=` | key | image bytes (PNG/JPEG, under data/screenshots only), or 404 | drawer picture (L2314) |
| GET `/api/learn/next?topic=` | optional topic | `{card: {id, topic, question, offer} \| null}` | drill (L2528) |
| GET `/api/learn/answer?id=` | id | `{answer}` or 404 | drill (L2541) |
| POST `/api/approve` `/api/resolve` `/api/confirm` | `{key}` | `{ok, message}` (200, or 409 refused) | card buttons |
| POST `/api/mark` | `{key, state, note \| null}` | `{ok, message}` | drawer "Registrar una respuesta" (L2402) |
| POST `/api/task/done` | `{id}` (number) | `{ok, message}` | "Hecha" |
| POST `/api/learn/grade` | `{id, grade 0..5}` | `{ok, message}` | drill (L2552) |
| POST `/api/stop` `/api/resume` | `{}` | `{ok, message}` | header STOP |

Every request carries `X-Panel-Token` (from `?t=`, read once and removed from the address, L651-656); a POST is
`application/json`. 401/403 mean the key changed. `message` is Spanish text written by the server and is shown as
received (toasts). Network failure becomes status 0 with "No se pudo hablar con el panel: ¿se cerró jobbot panel?".
The demo replaces the whole layer with `window.__DEMO__.api(path, body)` returning `{ok, status, data}` (L823).
Caps: lists of tasks/emails/needs/approvals are cut at 50 and board columns that are not live at 25; `total` says
how many exist ("y N más…" under every list that can be cut). Live columns (`submitted`..`offer`, `ghosted`) are never cut.

## 2. Shared parts

### 2.1 Refresh cycle (L851-880, 877, 882-902, 3011)
- `refresh(force)`: reads the snapshot; a content key (the snapshot without `generated_at`) equal to the last one
  and a clock less than 10 min ahead (`REDRAW_MS`) draws nothing; never redraws while an input, textarea or select
  inside `#app` has the focus (the next poll tries again) unless forced. Forced after every action and at boot.
- A new picture gives `fresh` marks (`mail:`, `task:`, `card:`) that rooms show for 4 s (`.fresh`), lines for the
  board to announce (max 4) and, when an application moved to interview or offer, the "ceremony".
- `act(path, body, button, onOk)`: disables the button, toasts the server's message (fallback "Hecho."/"No se pudo."),
  runs `onOk`, then for approve/confirm shows the button as "Aprobada"/"Anotada" in the success colour for 600 ms
  (L883-896, skipped under reduced motion or a hidden page), then forced refresh, then re-reads the open drawer.
- Polling only while the page is visible. On `pagehide` or becoming hidden the visit time is stored (L1303-1307).

### 2.2 Header controls (L1642-1667) `#controls`
| Item | Source | Behaviour |
| --- | --- | --- |
| `#run-state` badge | `controls.stopped` | "STOP activo" (bad) / "En marcha" (ok) |
| `#stop` (danger) / `#resume` | same | POST stop/resume, no confirmation, toast, refresh |
| `#open-palette` "Buscar" + kbd | none | opens the palette (2.8) |
| `#theme` "Tema" + kbd T | none | cycles theme (2.11) |
| `#open-keys` "?" | none | toggles the shortcut help (2.9); hidden under 700 px |
| `#b-info` (board) | `controls.window_open/next_window`, `claude` | window text: open / "Ventana cerrada, abre 18:00" (hour only when today in Santiago, else "07/10 18:00") / "sin datos" (null); spend "Claude, 24 h: US$1.84 de US$5.00" (budget optional); a green or gold LED |
Under 700 px the controls wrap below the tabs.

### 2.3 Board (L623-631, L953-1245)
- Four split-flap counters, each a button (`[data-b=turn|approve|wait|stale]`, role button, Enter/Space) with a hidden
  plain number (`[data-sr=...]`) for screen readers; capped at 99; "zero" dims. Numbers: `turn` = `turnCount` (the
  nav's Hoy badge, the title and the favicon), `approve` = `awaiting_approval.total`, `wait` = processes with
  turn "wait", `stale` = stale processes. They are doors (`jump`, L2763): turn -> Hoy and locate the ticket;
  approve -> Hoy, open fold `dec-approve`, locate it; wait/stale -> Procesos with that filter, locate the filters.
  A door opens "like a sun" from the number pressed (view transition kind `sun`, aimed at the click).
- Clock "Hora en Santiago" (flap, h:mm, ticks every second) and the 24 h dial (L1056-1121): midnight at the top,
  the bot's window 18:00-09:00 (hard-coded) lit up to now, busy hours 09:00-18:00 hatched, hand on now; on the first
  draw the hand runs from 18:00 to now. Hidden under 700 px. `aria-hidden`.
- Rotating line (34 flap cells, every 7 s, paused while hidden): STOP ACTIVO / VENTANA DE ENVIO ABIERTA / ABRE hh:mm,
  the first dated task with its countdown, "OFERTA DE x ESPERA", "ENTREVISTA CON x", else "TODO AL DIA" (L1133-1154);
  announcements (new mail, task, card moved, "DESDE TU ULTIMA VISITA n NOVEDADES") jump the queue and hold 6.5 s.
  `#b-line-sr` (role status) repeats the line as text. Flaps are drawn by CSS from `data-c`: no text in the DOM.
- Platform edge `#platform[data-mood]` (calm blue, turn gold, urgent red, stop gold hatch) from `moodOf`; `.ceremony`
  sweeps a light once (2.2 s). `--mood` custom property on `<html>`.

### 2.4 Nav / tabs (L636-641, L1669-1686, L2985-3001)
`nav.tabs[role=tablist]`, buttons `[data-tab=hoy|procesos|cola|learn]`, ids `tab-hoy|tab-procesos|tab-cola|tab-learn`,
`aria-selected`, roving `tabindex`, `aria-controls=app`; `#app[role=tabpanel][aria-labelledby=tab-<name>]`. Count badge
per tab: Hoy = `turnCount` (gold `.hot` when > 0), Procesos = live processes, Cola = scored + prepared totals, Aprender
= `learn.due`; the badge pops when its number changes. Arrow keys wrap, Home/End jump, focus follows. A sliding ink bar
(`.tab-ink`) is repositioned on render, resize and font load. The fourth tab is `learn` in the old address and DOM.

### 2.5 Ticket (L1309-1469) `section#ticket` in Hoy
One card for the first of `candidates` (order in `core/model.js`), skipping the ones put aside with "Después"
(`[data-ticket-skip]`; when all are skipped the set clears). Parts: stub (kind, big word/number, sub line), title
(company — title or task text), why line, buttons, "Después de esta: N pendiente(s)." / "Es la última.", "Despachadas
en esta visita: N." Tones (`data-tone`): turn (gold), approve (blue), late (red). `data-ticket=<id>`.
Buttons (`[data-ticket-act]`): "Hecha" for a task (two-step, closes it), the card's actions (approve/confirm are
two-step), "Abrir" `[data-ticket-open]` (opens the drawer of the application, from the card or the task's owner), link
"Abrir enlace" (task) / "Ver el aviso" (card) only for http(s). Doing something "punches" the ticket (hole, 300 ms)
before the request. With nothing left: "Andén despejado" + "Nada espera tu turno ahora." or "Despachaste N pendiente(s)
en esta visita. Nada más espera tu turno." and a ceremony if something was dispatched. New ticket slides in; tilts a
few degrees toward a fine pointer. Counts dispatched (`punched`) and skipped live only in the visit.

### 2.6 Drawer "detalle" `aside#detail` (L2300-2522)
Opened by any `[data-open]` title, a car, a night dot, a palette row, a ticket "Abrir", the address `#tab/<key>`.
Content, in order (all from `detail` unless noted):
1. Title "company — title"; step buttons `#seq-prev` `#seq-next` with "n de N" (`aria-live`) over the applications
   visible behind it (`[data-open]` in `#app`, in DOM order, wrapping); `#close-detail`.
2. State chip, "Puntaje {card.score}, {location}[, remoto]"; salary "Sueldo publicado: US$a a US$b al mes" or "no publicado".
3. Links "Ver el aviso" (`card.url`) and "abrir el formulario" (`apply_url`), http(s) only.
4. Journey `#journey` (L2421): the five layers (aviso, puntaje, cv, respuestas, compuerta; `li[data-layer].j-done|j-robot|
   j-you|j-todo`, `aria-current=step`) with, per done layer, a detail line (saved date and source, score and threshold,
   CV ready, form complete, approved/passed); the current layer shows `text` and the card's action buttons right there;
   a sent application shows all five done, "Después del envío" and the five-stop stepper, then the buttons. A closed
   one shows only the buttons.
5. `reason_short` (only when not on the way) and a fold "Texto completo del motivo" (`reason`); "Nota: …" when there is a
   note and no reason.
6. "Tareas abiertas de esta postulación" (snapshot tasks with this `job_key`, with countdown) and "Correos de esta
   postulación" (kind: subject, ago).
7. Screenshot `#shot-slot` when `card.has_screenshot`: "Cargando la captura…" then an `<img>` from a blob URL fetched
   with the token; else "Sin captura de prueba en seco." URL revoked on close/change.
8. "Registrar una respuesta" when `card.marks` is not empty: `#mark-note` (maxlength 200, text kept in `state.note`) and
   one `[data-mark=<state>]` button per mark; withdrawn/rejected/accepted are two-step. POST mark with the note.
9. "Historial" timeline (`events`, newest first: when, "from a to", note), "Cómo se calculó el puntaje" (bars of
   `score_detail`, any factor names from the server), "Para repasar de esta oferta" (`gaps`), "Dossier" (open, `pre`)
   or "Sin dossier todavía…", fold "Descripción del aviso".
Behaviour: opens by view transition growing from the origin element (title/car/button); focus moves to the drawer and
returns to the element that opened it (or its title) on close; pushes one history entry (`#tab/<key>`), so Back
closes it first; Escape closes; `j`/`k` (or `]`/`[`) step; stepping slides the content (kinds next/prev). It is not
redrawn by a refresh (it would lose picture, note and folds); it is re-read after every action, and a failed read
closes it with a toast "No se pudo abrir la postulación.".

### 2.7 Toast (L813-820) `#toast[role=status]`
One message at a time, 6 s, `.show` + `.ok|.bad`, bottom centre. Fed by actions, refresh failures, theme change.

### 2.8 Palette (L2902-2983)
Ctrl+K (any time) or `/` (when not typing). `div.pal-back > .pal[role=dialog][aria-modal]`, `#palette-input` (combobox),
`#palette-list` (listbox, `.pal-item`). Items: the four sections ("Ir a") and every card on the board (kind = state label,
text "company — title", extra = source). Filter: every word of the query contained in `norm(kind + " " + text)`; 12 max;
"Nada coincide.". Arrows move, Enter runs, Esc closes (and does not close the drawer), Tab is swallowed, mouse
move selects, click runs, click outside closes; focus returns to the previous element. Footer "Flechas para moverte…".

### 2.9 Keyboard layer (L2778-2892, L2844-2868)
| Key | Does |
| --- | --- |
| `1`-`4` | Hoy, Procesos, Cola, Aprender (new: `5` Sistema) |
| `j` `k` | move the list cursor (`li.cur` on `li[data-card]`/`li[data-task]`, kept across redraws by `state.cursor`); with the drawer open, next/previous application (`]` `[` too) |
| `Enter` | with a cursor and the body focused: open the application, or focus the task's "Hecha" |
| `Esc` | close help, else drawer, else clear cursor |
| `Ctrl+K`, `/` | palette |
| `t` | theme |
| `?` | shortcut help `.keys[role=dialog]` (click closes) |
Ignored while an input, textarea, select or contenteditable has the focus (except Esc) and with Ctrl/Meta/Alt (except
Ctrl+K) or while the palette is open. `popstate`: Back/Forward walk sections and close/open the drawer (`ignorePop` counter
for the history step that closing takes back, L2894).

### 2.10 Title, favicon, since-last-visit
- `document.title` = "(n) Panel" (n = `turnCount`, omitted at 0); it never names the purpose (the browser may be shared).
- Favicon (L1192): 64x64 canvas, dark tile, the count (99 max), a bar in the mood colour, as a blob URL on a `<link rel=icon>`
  created with `document.createElement` (`h()` refuses `link` and blob hrefs on purpose: do this one with the raw DOM).
- Diffs (L1255-1307): new mail / task / card moved -> `fresh` + announcement; first picture of a visit uses
  `localStorage panel.lastVisit` ("DESDE TU ULTIMA VISITA n NOVEDADES" counting mails and sent/closed cards updated since).

### 2.11 Themes (L8-16, L2847-2857, CSS L27-63)
Before first paint: `?theme=dark|light` wins, else `localStorage panel.theme`, else the system. `T`/`#theme` cycles
auto -> light -> dark -> auto (auto removes the key and the attribute), toast "Tema claro". Colours are OKLCH-derived tokens
(`docs/COLOR.md`): blue ink, amber = your turn, green = confirmed, red = late.

### 2.12 Motion (all skipped when `still()` = reduced motion or hidden page; the page works without it)
View transitions with kinds `fwd`/`back` (section travel, 0.56 s + 0.12 s per extra stop, plus a light riding the platform
from tab to tab, L2647), `sun` (door), `open`/`close` (drawer from/to its origin), `next`/`prev` (stepping). Rows enter one
after another (`--i`, max 16, class `entering` 1.8 s) and funnel numbers count up (`--n`). FLIP glide of `[data-card]`
and `[data-task]` rows when a refresh or filter reorders them (340 ms, skipped if moved more than 1.5 screens). Rings
and bars fill on first draw, cars glide to a new stop, button "sending"/"sent". `body.away` stops looping animation.

### 2.13 Focus and folds (L1723-1758)
A redraw replaces every node, so `focusMark` finds the focused element again by id, or by card + one of `data-done|
data-open|data-action|data-filter|data-grade|data-ticket-act|data-ticket-open|data-ticket-skip|data-car`, or a fold
summary, and restores focus and caret. Folds are `details.group[data-fold=<id>]`; their open state is remembered in
`state.folds` and saved with the view. Hovering/focusing a car or a row lights its twin (`.hl`, L2829).

## 3. Hoy (L1869-1885, tab `hoy`) `div#today` (first child of `#app`)
Wide: ticket and the day's list in the left column, decisions in the right; narrow: one column.

| Block | Shows (field) | Actions | Empty / limits |
| --- | --- | --- | --- |
| Ticket (2.5) | `candidates` | see 2.5 | "Andén despejado" |
| "Lo que hizo jobbot", last 24 h (`#night`, L2043) | `activity`: bars = `slots[].seen` per half hour, hatched = `slots[].open === false`, dots = `moves[]` (gold if the card waits for Gonza, green if `to` = submitted), now line, hours every 3 h; sentence `#night-line`; legend; fold `night-moves` listing moves (hour, "(ayer)", title button `[data-night-open]`, state chip) | dot/title -> drawer | not drawn without slots; "Sin avisos nuevos ni movimientos en las últimas 24 horas."; "Ver los últimos N de M movimientos" when cut at 60 |
| "Tu turno" (`#today-summary`, L1792) | `today.tasks[]`: text, owner chip (company: state, via `ownerOf`), link, countdown (`when late|soon`) | "Hecha" `[data-done=id]` two-step | "No tienes tareas abiertas." / "Sin tareas abiertas."; "y N más…" |
| "Esperan tu decisión" (`#decisions`, L1841) | `needs_gonza` + `awaiting_approval` in folds by kind; per card: title button, short reason, long reason fold, "Puntaje N en source", actions, "Ver el aviso" | approve/resolve/confirm per `card.actions` (approve, confirm two-step) | "Nada pendiente."; "Por aprobar: nada por aprobar."; fold open by default when 7 or fewer |
| "Respuestas de los últimos 7 días" (`#recent`) | `today.emails[]` (not acks): kind chip, company or sender, subject, summary, "hace 3 h, 07-10-2026 14:05 · from" | none | "Sin respuestas nuevas."; "y N más…" |
Task rows carry `.item.late|now|next|fresh` (next = the ticket's task). The summary line: "N tarea(s) abierta(s)[, M vencida(s)].
La más cercana: vence en 30 h." (nearest by the server's order, not overdue).

## 4. Procesos (L1944-1994, tab `procesos`)
- `h2`, `#metrics-line` (L1964): "Últimos 7 días: N enviadas, N respuestas (pct), N pruebas, N entrevistas. De entrevista a oferta: pct." from `metrics.*`; "n/a" for a null ratio.
- Funnel (`#stations`, 5 `.station[data-station=<plural label>]`, L1889): count `funnel.*`, label, "pct de las enviadas…" (each stop a share of the one it came from; tests and interviews both of the answers; offers of interviews; nothing when the base is 0).
- Line map (`svg.map`, L1533): river sized by the funnel counts, one car per live process at its stop (`[data-car=<key>]`, role button, tabindex 0, aria-label "company — title: state"), classes mine/check/ghost (`ghosted` goes to the siding "Sin respuesta", 20 max), a red dot on a car with a late task, `.fresh` pulse; click/Enter/Space opens the drawer. Legend: te toca, define el siguiente paso, espera a la empresa, sin respuesta.
- Filters (`[data-filter=all|mine|wait|stale]`, `aria-pressed`, counts; "Te tocan" = mine + check): saved in the view.
- Rows `li.proc[data-card][data-state][data-turn]` (L1923): name button `[data-open]` + source; five-dot stepper (ghosted shows a chip); time since `updated_at` with a ring (fills over 10 days; warm at 70 %, hot at 100 %, "hoy" / "N días [sin noticias]"); `note` ("Sin notas"); next step (task text + countdown, "Esperar su respuesta", "Sin noticias: decide si insistes", "Sin tarea: agrega el siguiente paso", "Decide la oferta"); turn chip (Te toca / Define el paso / La empresa). Header row hidden under 980 px.
- Empty: "Todavía no hay procesos abiertos." / "Ninguno cumple este filtro."
- Fold `closed` ("Cerradas", count): `div[data-column=accepted|rejected|withdrawn]` with the queue rows, "Ninguna.", "y N más…".

## 5. Cola (L2005-2018, tab `cola`)
Lead "Lo que jobbot encontró y preparó, de mejor a peor puntaje. Todavía no se envió." Then, each only when its data exists:
- "Del aviso al envío" `#route` (L2169): needs `pipeline`; five counts `#route-line .station-n[data-layer]` (`seen`, `above`, `with_cv`, `complete`, `sent`), shares, the river, a sentence naming the threshold.
- "Tu umbral" `#threshold` (L2211): needs `pipeline.threshold`, `seen > 0`, 101 `score_counts`; 81-bar histogram (scores 20..100, the first bar "20 o menos"), the real threshold line, a slider `#thr-range` 40-100 to try another (what-if only: nothing is written; the threshold lives in `profile/criteria.toml`), `#thr-out`, `#thr-reset`, notes `#thr-in` "N pasan" / `#thr-out-note` "N quedan fuera", `#thr-say` (role status). The tried value survives redraws (`state.whatIf`).
- "Confianza" `#trust` (L2277): needs `trust`; review phase: `first_n` boxes (`.t-box.done` per `reviewed`) and "Llevas N de M envíos aprobados por ti…"; after: "Ya envía solo… tope de N por día"; meter `sent_24h` of `cap`.
- Columns `section[data-column=prepared|scored]` (label, total): rows `li.queue-row[data-card][data-state]` = score, title button, "source · note", mini path of five dots + layer name + text (`.mini[data-layer]`), action buttons; "Ninguna."; "y N más…".

## 6. Aprender (L2524-2604, tab `learn`)
`div.learn`: `h2`, `#learn-stats` "Vistas {seen} de {total}, pendientes hoy: {due}". Drill (`#topic` select "todos los temas", `#next-card`
"Siguiente pregunta"/"Otra pregunta"): card shows topic (+ ", de la oferta X"), `#question`, `#my-answer` textarea
("no se guarda", kept in `state.drill.text` across redraws), `#reveal` "Mostrar la respuesta" -> `pre#answer` + "¿Cómo te fue?
0 = en blanco, 5 = perfecto." + six `[data-grade=0..5]` buttons -> POST grade, toast with the server's text (SM-2 schedule),
forced refresh, card cleared. `#drill-empty`: "Estás al día: no hay preguntas pendientes." / "Elige un tema o pulsa «Siguiente
pregunta».". "Temas débiles" (`weak_topics`: topic, "promedio 2.1"), fold `progress` table (`topics`: seen/total, "N pendientes"),
"Huecos que salen de las ofertas" (`gaps`: "topic, offer", "repasada"/"por repasar").

## 7. Routing, storage and hooks
- Address: `#hoy`, `#procesos`, `#cola`, `#learn`, plus `/<job key>` for an open drawer; written with pushState for a
  section change and for opening the drawer, replaceState otherwise; none when embedded (`?embed` or inside a frame).
- Storage: `sessionStorage panel.view` `{tab, filter, folds}` (ignored when embedded); `localStorage panel.theme`
  (`dark|light`, absent = auto), `panel.lastVisit` (ISO time). New: `panel.lang`, sound and settings keys.
- Hooks other code relies on: `#app`, `#detail`, `#toast`, `#platform`, `[data-tab]`, `[data-card]`, `[data-task]`,
  `[data-open]`, `[data-done]`, `[data-action]`, `[data-mark]`, `[data-filter]`, `[data-grade]`, `[data-fold]`, `[data-column]`,
  `[data-station]`, `[data-car]`, `[data-layer]`, `[data-b]`/`[data-sr]`, `[data-ticket*]`, `[data-night-open]`, `[data-email]`, `li.cur`.

## 8. Legacy quirks kept on purpose
Items 1-7 carry a `// legacy quirk:` comment in the port (`core/model.js`, `diff.js`, `session.js`, `geometry.js`, `format.js`,
`dom.js`). Items 8-10 are old behaviours ported unchanged or left to the
keyboard layer (not part of P3), listed so nobody "fixes" them by accident.
1. A reason kind the page does not know belongs to no group: not in the ticket queue nor Hoy's list, yet `turnCount` counts it (via `needs_gonza.total`).
2. `sinceLastVisit` only marks sent/closed cards; a newly prepared or stuck one is not "news".
3. The 4 s clearing of the `fresh` marks is per picture: a second picture within 4 s loses its marks early.
4. The dial's night window is the constants 18:00-09:00, not `schedule.toml` (the strip uses `slots[].open`).
5. `initials("")` is "" (the "?" fallback is stripped as a symbol).
6. Polling goes on after a 401 (now with the longer waits of the backoff).
7. `restoreFocus` asked every input for its caret and threw on a range slider; the port catches it.
8. `ownerOf`: a text naming "Acme" and "Acme Labs" names two applications and pins none.
9. A process "check" turn includes replied/test/interview with no task (not only ghosted): they all say "Define el paso".
10. `Esc` inside the palette stops there; elsewhere it closes help, then the drawer, then the cursor.

Dead or unused (checked by search): `.ticket::before{content:none}` (L285; no other rule gives `.ticket` a `::before`, so it
resets nothing); `--gx`, `--gy`, `--glare` set by the ticket's tilt (L1435-1440) and read by no CSS rule (only `--rx`/`--ry` are);
`h("div", {class: "row", style: null})` (L2566; `style: null` is a no-op). Not dead although they look it: `.chip.check/.wait`
(turn chips), `.b-line .fl` (the flaps), `STATUS_WORD.todo` (spoken for every journey step not yet reached).

## 9. What the browser tests assert (`tests/panel_browser.rs`, 35 tests, all `#[ignore = "needs chromium"]`)
Seed ("Env::start"): test:1 needs Gonza (hostile title), test:2 prepared with a screenshot, test:3 sent; tasks (one overdue
with hostile text, one dated, two undated, one with a `javascript:` link); two emails (one hostile, one ack).
"full" adds `criteria.toml` and `schedule.toml`.

| Test | Hooks and behaviour it needs |
| --- | --- |
| the_page_draws_the_decisions_and_hostile_text_stays_text | `[data-card]` >= 2; `location.search` empty after boot; hostile text visible as text, no `img[src=x]`, `window.pwned` undefined; text "2 postulaciones esperan", "Por aprobar"; `[data-tab=hoy] .count` = "6" (4 tasks + approval + form) |
| the_today_list_comes_first_and_what_an_email_says_stays_text | `#today` is `#app`'s first child; texts "Tu turno", "4 tareas abiertas, 1 vencida", "La más cercana: vence en 30 h", "Venció hace 2 h", "Abrir enlace", "Respuestas de los últimos 7 días", "Reclutador"; acks excluded; only svg in `#today` is `.night-svg`; `[data-task] button` = 4 all `[data-done]`; `[data-task] a` hrefs = only the https one; no `a[href^=javascript]` |
| a_task_is_closed_from_the_page_only_with_a_second_click | `[data-task]` x4; `[data-done=id]` two-step (second click after 500 ms); `#today-summary` updates ("3 tareas abiertas") |
| the_processes_tab_says_where_each_one_is_and_whose_turn_it_is | `[data-station="Enviadas"|"Entrevista"|"Respondieron"] .station-n/.station-c` ("0% de las enviadas", "" for empty base); `[data-card] [data-turn]` wait/mine; row text "Esperar su respuesta", "La empresa", "Te toca", task text, "vence en 5 h"; `[data-filter=wait|mine]` filter rows |
| approving_from_the_page_lands_in_sqlite | `[data-card=test:2] [data-action=approve]` two-step; disappears after |
| a_double_click_only_arms_a_permanent_action | two `click()` in one tick: label contains "Seguro", nothing sent |
| confirm_and_a_terminal_mark_need_a_second_click | `[data-action=confirm]` two-step; `#detail [data-mark=withdrawn]` two-step; `[data-column=withdrawn] [data-card=test:3]` appears |
| the_open_drawer_keeps_its_screenshot_and_note_when_the_page_redraws | `[data-card] [data-open]`; `#detail img` loaded; `#mark-note` value survives tab changes; `#next-card` exists in the 4th tab |
| a_refused_mark_keeps_the_typed_note | `#detail [data-mark=replied]`; toast text includes "No se pudo marcar"; drawer re-read shows `[data-mark=interview]`; `#mark-note` keeps text |
| the_drawer_shows_the_screenshot_and_records_an_answer | `#close-detail`; mark with note -> `[data-card=test:3][data-state=replied]` |
| escape_closes_the_drawer | `#detail.open`; keydown Escape on `document` closes |
| no_tab_and_no_drawer_prints_a_stray_null_or_object | no "null", "undefined", "[object", "NaN" in body text on any tab or in the drawer; each tab draws > 40 chars |
| the_drill_grades_a_card_and_sm2_schedules_it | `[data-tab=learn]`, `#next-card`, `#question`, `#answer` absent until `#reveal`, `[data-grade=5]`, toast includes "nota 5" |
| stop_and_resume_work_from_the_page | `#stop`, `#run-state` text "STOP activo"/"En marcha", `#resume` |
| a_wrong_token_says_so_in_plain_words | body text contains "clave" |
| an_empty_panel_shows_empty_states_and_na | texts "Nada pendiente.", "No tienes tareas abiertas.", "Sin tareas abiertas.", "Sin respuestas nuevas.", "n/a", "Todavía no hay procesos abiertos."; 5 `[data-station]` |
| the_tabs_move_with_the_arrow_keys | `#tab-hoy`..`#tab-learn` ids, ArrowRight/Left/Home keydown on the tab, focus follows, `tabindex` -1 on others, `#app[aria-labelledby=tab-procesos]` |
| a_redraw_keeps_the_keyboard_focus_on_the_same_button | `[data-filter=mine]` keeps focus after the redraw it causes |
| the_ticket_puts_the_most_urgent_thing_first_and_a_task_closes_with_a_second_click | `.ticket[data-tone=late][data-ticket]`, `.ticket .t-title`, `#ticket` has no img/svg, `[data-ticket-act=done]` two-step, `.ticket .t-after` contains "Despachadas en esta visita: 1" |
| the_board_counts_what_waits_and_the_tab_and_the_platform_follow | `[data-sr=turn|approve|wait]` = "6","1","1"; `document.title` "(6) Panel"; `#platform[data-mood=urgent]`; `.fl` has no text |
| the_palette_opens_from_the_keyboard_and_jumps_to_an_application | Ctrl+K on `document`, `#palette-input`, `.pal-item` (1 match for "sent": "Co 3", "Sent role"), Enter opens `#detail`, `/` reopens, Escape closes the palette only |
| the_line_map_has_one_car_for_each_open_application | `[data-car]` count = `li.proc` count; role button; click opens drawer; no `svg [data-card]` |
| with_nothing_to_do_the_platform_is_clear_and_calm | text "Andén despejado", `#platform[data-mood=calm]`, no `.ticket`, title "Panel" |
| the_board_numbers_are_doors_to_the_lists_they_count | `[data-b=wait]` role button; click -> procesos with `[data-filter=wait][aria-pressed=true]`; `[data-b=approve]` -> hoy with `[data-fold=dec-approve]` open |
| the_address_follows_the_sections_and_back_closes_the_drawer | `location.hash` `#hoy`, `#procesos`, `#procesos/<key>`; history.back closes drawer, then leaves the section |
| the_drawer_steps_through_the_applications_and_gives_the_focus_back | `#detail .seq span` "n de N", `j`/`k` change `#detail h2`, Escape closes and focus is on a `[data-open]` |
| the_number_keys_and_j_k_move_around_the_page | keys `2`,`4`,`1` select tabs; `j` puts `li.cur` on `#app li[data-card],li[data-task]`; Escape clears; `?` shows `.keys`, Escape hides |
| the_theme_follows_the_key_the_button_the_address_and_is_remembered | `data-theme` on `<html>` (absent = auto), body background rgb(237,240,246) light / rgb(10,16,28) dark, `t` key, `#theme` button cycle, `localStorage`, `&theme=dark` in the address |
| sections_travel_along_the_line_and_a_door_opens_a_sun | `.b-dial .dl-tip` and `.dl-hand[transform=rotate(`; click on `[data-tab=cola]` sets `html[data-vt=fwd]`, `--vt-time: 0.68s`, one `.platform .rider`; `#app.entering`; back is `back 0.56s`; `.station-n` text equals its `--n`; `[data-b=approve]` click sets `data-vt=sun` and `--vt-r` in px |
| an_approval_confirms_itself_before_the_list_closes_over_it | `button.sent` with text "Aprobada" before the button disappears |
| hoy_says_what_jobbot_did_in_the_last_day_and_each_move_opens_its_application | `#night-line` text "guardó 3 avisos nuevos" and "registró N" = `.n-dot` count; `[data-fold=night-moves]` list; `[data-night-open]` opens the drawer; hostile title as text |
| with_nothing_stored_the_last_day_is_empty_and_says_so | `#night-line` exact text "Sin avisos nuevos ni movimientos en las últimas 24 horas."; 0 `.n-dot`; in Cola `#route` exists, `#threshold` and `#trust` do not |
| cola_draws_the_path_to_a_send_and_a_threshold_you_can_try_without_changing_it | `#route-line .station-n` = [13,3,2,2,1]; `#thr-range` 60; `#thr-in` "3 pasan"; `#thr-out-note` "10 quedan fuera"; after input 50: `#thr-say` "Con 50 pasarían 6 de 13 avisos: 3 más que con 60" and "no cambia nada"; `.h-if[visibility=visible]`; `#thr-reset` shown, resets; `criteria.toml` untouched; `[data-card=test:2] .mini[data-layer=compuerta]` |
| trust_counts_the_sends_you_approved_and_the_cap_of_the_day | `#trust`; `.t-box` x5, `.t-box.done` count; texts "Llevas 0 de 5", "1 de 10", "Llevas 1 de 5" |
| the_drawer_starts_with_the_path_of_the_application_and_the_buttons_sit_where_it_stopped | `#journey li.j-done` count (4 for the prepared one, 5 for the sent one), `li[data-layer=compuerta].j-you` holding `[data-action=approve]` once, `#journey .j-after` for a sent one |

Every assertion on Spanish text must be rewritten on purpose for the new wording or run with `?lang=es`; the `learn` tab id,
`#app`, `#platform` and the flap/dial hooks only survive if the new DOM keeps them (the spec keeps `data-tab`, `data-card`,
`data-task`, `data-open`, `#detail`, `#app`).
