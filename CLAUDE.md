# Booking Agent

Rust-based SMS booking agent for freelancers. Handles appointment scheduling via natural language SMS, with a web admin UI. Uses Ollama (local AI), Twilio (SMS), SQLite (storage), and .ics calendar invites.

## Commands

- `cargo build --release` — build optimized binary
- `cargo run` — run dev server (default port 3000)
- `cargo test` — run all tests
- `cargo clippy` — lint
- `cargo fmt` — format

## Architecture

- **Framework**: Axum 0.7 (async, tokio runtime)
- **AI**: LlmProvider trait with Ollama (default), Groq, OpenAI-compatible backends
- **Messaging**: MessagingProvider trait — Twilio SMS for MVP, WhatsApp and others later
- **Twilio**: BYOA (Bring Your Own Account) — user creates their own Twilio account, buys a number (~$1/mo), registers A2P 10DLC, and enters credentials in admin UI. We do NOT manage Twilio on behalf of users (no ISV subaccounts for MVP).
- **Admin UI**: Single HTML page at `/admin`, token-auth, embedded via `include_str!`
- **Database**: SQLite via rusqlite (bundled), migrations in `migrations/`
- **Calendar**: .ics file generation sent via SMS, no Google OAuth
- **Pricing**: $39 one-time (self-hosted), $7/mo (cloud-hosted, we run the server). User pays Twilio directly in both tiers.

See `docs/spec.md` for the full MVP specification, including database schema, conversation flow, and deployment instructions.

## Project Structure

- `src/handlers/` — HTTP route handlers (webhook, admin, health)
- `src/services/ai/` — LlmProvider trait + implementations (ollama, groq, openai)
- `src/services/messaging/` — MessagingProvider trait + implementations (twilio_sms, future: whatsapp)
- `src/services/calendar.rs` — .ics generation + booking logic
- `src/models/` — Booking, Intent, Conversation, User structs
- `src/db/` — SQLite layer
- `src/web/admin.html` — admin UI (embedded in binary)
- `migrations/` — SQL migration files

## Code Style

- Error handling: `thiserror` for typed errors in libraries, `anyhow` for application-level
- All external API clients behind traits (testability, swappability)
- Async everywhere (tokio runtime)
- Prefer `reqwest` for all HTTP calls (Twilio, AI providers, etc.)
- No `unwrap()` in production code — propagate errors with `?`
- Structs derive `Serialize`/`Deserialize` where needed
- Use `tracing` for logging, not `println!`

## Key Patterns

- Phone number is the session key for multi-turn conversations (30min TTL)
- All bookings stored in SQLite — it is the source of truth, not an external calendar
- Confirmation-based flow: AI always proposes a time and waits for user confirmation before booking
- Reply routing: customer messages forwarded to owner, owner replies routed back through Twilio
- Admin commands via SMS use `#` prefix (`#pause`, `#resume`, `#status`)
- One Twilio number handles SMS + voice + WhatsApp (same number, different channels)
- User onboarding: create Twilio account → buy number → register A2P 10DLC → paste credentials in admin UI
- MessagingProvider trait abstracts the transport layer (SMS today, WhatsApp later)

## Rate Limiting & Cost Protection

Twilio has no spending cap, no inbound blocking, and charges for every message received. All protection must be built into our app.

- **Global rate limit**: max 100 inbound messages per hour per Twilio number. When exceeded → pause agent, stop replying, alert owner.
- **Per-customer rate limit**: max 15 messages per phone number per hour. When exceeded → auto-block that number, alert owner.
- **Monthly spending estimate**: track message count, estimate cost, alert owner at configurable threshold (default 80%).
- **Monthly message budget**: configurable in admin UI. When reached → pause agent or alert only (user's choice).
- **Blocklist**: owner can block numbers via `#block 555-1234` SMS command or admin UI. Blocked numbers are ignored (no outbound reply sent).
- **Auto-block**: numbers exceeding per-customer rate limit are auto-blocked and owner is notified.
- **Twilio cannot block inbound charges** — we can only save on outbound by not replying. Worst case for 100 spam messages in an hour: ~$0.80 in unavoidable inbound charges.
- Store rate limit counters in SQLite (`messages_this_hour` table with phone_number + received_at).

## Docs

- `docs/spec.md` — Full MVP spec, schema, flows, deployment, roadmap
- `docs/database.md` — Schema reference (create if needed during development)
- `docs/prompts.md` — AI prompt templates and intent extraction format (create during development)

## Don't

- Don't add Google Calendar OAuth — we use .ics invites
- Don't use `google-calendar3` crate
- Don't auto-book without confirmation
- Don't expose owner's personal phone number to customers
- Don't use regex for intent parsing — always use the AI provider
- Don't manage Twilio accounts on behalf of users (no ISV subaccounts)
- Don't hardcode Twilio as the only messaging provider — use the MessagingProvider trait
- Don't send marketing/promotional messages via the agent — only transactional (booking confirmations, reminders) to stay within Twilio AUP
