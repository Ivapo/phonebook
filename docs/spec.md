# Booking Agent — MVP Spec & Roadmap

Rust-based SMS booking agent for freelancers. Customers text a Twilio number to book appointments; the owner manages everything via admin UI and SMS commands.

## Status Legend

- [x] Implemented and tested
- [ ] Not yet implemented

---

## Core Architecture

- **Framework**: Axum 0.7 (async, tokio runtime)
- **Database**: SQLite via rusqlite (bundled), WAL mode, foreign keys enabled
- **AI**: `LlmProvider` trait with swappable backends
- **Messaging**: `MessagingProvider` trait abstracting SMS/future channels
- **App UI**: Unified PWA at `/app`, token-auth, embedded via `include_str!`. Bottom tab bar (Inbox, Bookings, Settings), real-time SSE. `/admin` and `/inbox` redirect to `/app`.
- **Config**: Environment variables with defaults (`AppConfig` struct)

---

## Implemented Features

### SMS Webhook & Conversation Engine

- [x] POST `/webhook/sms` — receives Twilio webhooks
- [x] Twilio signature validation (skipped when `twilio_auth_token` is empty for dev)
- [x] URL reconstruction with `X-Forwarded-Proto`/`X-Forwarded-Host` for reverse proxies
- [x] Multi-turn conversation state per phone number (30min TTL, stored in SQLite as JSON)
- [x] Conversation states: Idle, CollectingInfo, Confirming, Rescheduling, Cancelling
- [x] LLM-based intent extraction (Book, Reschedule, Cancel, Confirm, Decline, GeneralQuestion, Unknown)
- [x] Dynamic info collection — LLM asks for missing fields (name, date, time)
- [x] Confirmation-based flow — never auto-books, always waits for customer to confirm
- [x] Reschedule support — cancels old booking, starts new flow with pre-filled info
- [x] Cancel support — finds most recent booking and marks cancelled

### Scheduling & Availability

- [x] JSON-based availability slots (day + start/end times)
- [x] Business hours validation — rejects bookings outside available hours
- [x] Conflict detection — prevents double-booking
- [x] Duration validation — ensures appointment doesn't exceed slot end time
- [x] LLM receives availability context in system prompt

### Booking Management

- [x] Booking CRUD in SQLite (create, read, list with filters)
- [x] Booking statuses: Pending, Confirmed, Cancelled
- [x] Fields: id, customer_phone, customer_name, date_time, duration_minutes, notes, status

### Calendar Integration

- [x] .ics file generation (RFC 5545 compliant)
- [x] GET `/calendar/:booking_id` serves .ics download
- [x] Calendar URL included in booking confirmation SMS

### App UI & Admin API

- [x] GET `/app` — unified mobile-first PWA with bottom tab bar (Inbox, Bookings, Settings)
- [x] GET `/admin`, GET `/inbox` — permanent redirects to `/app`
- [x] Header bar with agent status pill (Active/Paused), SSE connection dot, sign out
- [x] Token stored as `app_token` in localStorage, auto-migrates from `admin_token`/`inbox_token`
- [x] PWA: inline manifest, blob service worker, Add to Home Screen support
- [x] Bearer token auth on all `/api/admin/*` and `/api/inbox/*` endpoints
- [x] GET `/api/admin/status` — dashboard stats (paused, messages/hr, blocked count, upcoming bookings)
- [x] GET `/api/admin/bookings` — list bookings (filterable by status)
- [x] POST `/api/admin/bookings/:id/cancel` — cancel a booking
- [x] GET `/api/admin/blocked` — list blocked numbers
- [x] POST `/api/admin/block` — block a number
- [x] POST `/api/admin/unblock` — unblock a number
- [x] POST `/api/admin/pause` — pause agent
- [x] POST `/api/admin/resume` — resume agent
- [x] GET/POST `/api/admin/settings` — business name, owner name, timezone, availability, AI preferences

### Owner Inbox

- [x] Inbox tab — threaded conversation list with unread badges
- [x] Thread view — chat bubbles for customer messages, AI replies, and owner replies
- [x] Reply bar — owner can type and send replies directly to customers
- [x] GET `/api/inbox/threads` — list all conversation threads with unread counts
- [x] GET `/api/inbox/thread/:phone` — get messages for a thread
- [x] POST `/api/inbox/thread/:phone/read` — mark thread as read
- [x] POST `/api/inbox/reply` — send owner reply (injects into conversation + sends via messaging provider)
- [x] GET `/api/inbox/events` — SSE stream for real-time inbox updates (catchup + live)
- [x] SSE stays connected across all tabs, updates inbox badge when on other tabs
- [x] Desktop: sidebar thread list (320px) + thread view side-by-side
- [x] Mobile: thread view overlays list, back button to return

### AI Personality Preferences

- [x] `AiPreferences` struct with `from_json()` / `to_prompt()` and serde defaults
- [x] JSON column on `users` table — partial JSON works, NULL = all defaults
- [x] Preference categories: Identity (name, disclose AI, speak as business), Tone (professional/friendly/casual), Capabilities (book/cancel/reschedule/answer questions), Returning Customers (greet by name, remember prefs), Boundaries (booking only, share pricing + pricing info), Custom Instructions (free text)
- [x] `to_prompt()` generates personality instructions injected between system prompt and business context
- [x] Only non-default preferences emit prompt lines (minimal additions for default config)
- [x] Settings UI: structured inputs (text, checkboxes, radios) grouped into labeled subsections with own Save button
- [x] JSON validation on save — returns 400 for invalid `ai_preferences`

### SMS Admin Commands (owner sends from configured phone)

- [x] `#pause` — stop replying to customers
- [x] `#resume` — reactivate agent
- [x] `#status` — show active/paused, message count, blocked count
- [x] `#block <number>` — manually block a phone number
- [x] `#unblock <number>` — manually unblock a phone number
- [x] Owner-only enforcement — non-owner `#` messages go to conversation engine

### Rate Limiting & Cost Protection

- [x] Per-customer: max 15 messages/hour, auto-blocks on exceed
- [x] Global: max 100 messages/hour, pauses agent on exceed
- [x] Auto-blocking with owner notification
- [x] Manual blocklist (SMS + admin UI)
- [x] Silent ignore for blocked numbers (no outbound reply = no Twilio cost)
- [x] Hourly window cleanup
- [ ] Monthly message count tracking
- [ ] Twilio cost estimate calculation
- [ ] Configurable monthly budget in admin UI
- [ ] Budget threshold alerts (e.g. 80% warning)
- [ ] Budget enforcement mode (pause agent vs. alert only)

### LLM Providers

- [x] `LlmProvider` trait (async `chat` method)
- [x] Ollama implementation (default model: llama3.2)
- [ ] Groq implementation
- [ ] OpenAI-compatible implementation
- [ ] Model selection in admin UI

### Messaging Providers

- [x] `MessagingProvider` trait (async `send_message`)
- [x] Twilio SMS implementation (basic auth, form-encoded API)

### Owner Notifications

- [x] Alert on auto-block (rate limit exceeded)
- [x] Alert on global rate limit pause

### Testing

- [x] 29 unit tests (models, services, scheduling, AI preferences)
- [x] 27 integration tests (admin API, inbox API, webhook, SMS commands, calendar, rate limiting)

### Dev Chat UI

- [x] GET `/dev` — two-panel SMS simulator (Customer + Owner) for testing without Twilio
- [x] POST `/api/dev/message` — processes messages through conversation engine, returns replies as JSON
- [x] Reuses same conversation logic and admin commands as the webhook
- [x] Status bar auto-refreshes agent state every 5s
- [x] No auth required (dev-only tool)

### Other

- [x] GET `/health` — health check endpoint
- [x] Structured logging via `tracing`
- [x] Error handling with `anyhow`/`thiserror`
- [x] MIT license

---

## Not Yet Implemented (MVP gaps)

### Monthly Budget System

The hourly rate limiting is done, but monthly cost tracking is missing. Needed:

1. **DB table**: `monthly_usage` with columns for month, inbound count, outbound count, estimated cost
2. **Increment on each send/receive**: update counters in webhook handler
3. **Cost estimation**: inbound ~$0.0079/msg, outbound ~$0.0079/msg (US Twilio pricing)
4. **Admin UI widget**: show current month spend, budget bar, configure budget
5. **API endpoints**: GET `/api/admin/budget`, POST `/api/admin/budget` (set limit + mode)
6. **Threshold alerts**: notify owner at configurable % (default 80%)
7. **Enforcement**: pause agent or alert-only when budget reached (owner's choice)

### Alternative LLM Backends

The trait exists, just needs concrete implementations:

1. **Groq**: `src/services/ai/groq.rs` — HTTP POST to `api.groq.com`, API key auth
2. **OpenAI-compatible**: `src/services/ai/openai.rs` — works with OpenAI, Together, local vLLM, etc.
3. **Admin UI**: dropdown to select provider + model, fields for API key/URL

---

## Post-MVP Roadmap

Possible future features (not blocking MVP launch):

### WhatsApp Support
- Add `WhatsAppProvider` implementing `MessagingProvider`
- Same Twilio number handles both SMS and WhatsApp
- Channel detection in webhook (Twilio includes channel info)

### Booking Reminders
- Background task scheduler (tokio interval or cron-like)
- Send reminder SMS N hours before appointment (configurable)
- Reminder settings in admin UI

### Google Calendar Sync (read-only)
- Import busy times from Google Calendar to prevent conflicts
- No OAuth write — keep .ics as the outbound format

### Multi-language Support
- Detect customer language from SMS
- Pass language context to LLM for localized replies

### Analytics Dashboard
- Booking trends (daily/weekly/monthly)
- Response time metrics
- Customer return rate

### Multi-user / Multi-business
- Support multiple businesses on one instance
- Route by Twilio number to different configs
- Separate DBs or tenant isolation

### Voice Call Handling
- Basic IVR via Twilio voice webhooks
- "Press 1 to book" style flow, or voicemail-to-text

---

## Project Structure

```
src/
  config.rs          — AppConfig (env vars)
  state.rs           — AppState (db, config, providers, paused flag)
  handlers/
    webhook.rs       — SMS webhook, admin commands, rate limiting
    admin.rs         — App page handler + admin API endpoints
    inbox.rs         — Inbox API endpoints + SSE stream
    calendar.rs      — .ics download handler
    dev.rs           — Dev chat UI + message API
    health.rs        — Health check
  services/
    ai/
      mod.rs         — LlmProvider trait
      ollama.rs      — Ollama implementation
      intent.rs      — Intent parsing from LLM JSON
    messaging/
      mod.rs         — MessagingProvider trait
      twilio_sms.rs  — Twilio SMS implementation
    calendar.rs      — .ics generation
    conversation.rs  — Multi-turn conversation engine
    scheduling.rs    — Availability & conflict checking
    inbox.rs         — Inbox event recording + broadcast
  models/
    mod.rs           — Booking, BookingStatus, Intent, AiPreferences structs
    availability.rs  — AvailabilitySlot parsing & checking
    ai_preferences.rs — AiPreferences with from_json/to_prompt
  db/
    mod.rs           — init_db, migrations
    queries.rs       — All SQL queries
  web/
    app.html         — Embedded unified PWA (inbox + bookings + settings)
    dev_chat.html    — Embedded dev chat simulator
migrations/
  001_initial.sql    — Schema
  003_ai_preferences.sql — AI preferences column on users
tests/
  integration_tests.rs — Full integration test suite
docs/
  spec.md            — This file
```

---

## Deployment

### Self-hosted ($39 one-time)

1. Build: `cargo build --release`
2. Set env vars: `DATABASE_URL`, `ADMIN_TOKEN`, `TWILIO_*`, `OWNER_PHONE`, `OLLAMA_URL`
3. Run Ollama locally with llama3.2
4. Run: `./target/release/phonebook`
5. Point Twilio webhook to `https://yourdomain.com/webhook/sms`

### Cloud-hosted ($7/mo — we run the server)

Same binary, we manage hosting and Ollama. Customer provides their own Twilio credentials via admin UI.

### Twilio Setup (both tiers)

1. Create Twilio account
2. Buy a phone number (~$1/mo)
3. Register A2P 10DLC (required for US SMS)
4. Paste Account SID, Auth Token, and Phone Number into admin UI
