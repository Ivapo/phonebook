# Phonebook

SMS booking agent for freelancers. Handles appointment scheduling via natural language SMS, with a web admin UI.

Customers text your Twilio number to book, reschedule, or cancel appointments. A local AI (Ollama) interprets messages, manages multi-turn conversations, and confirms bookings. You get notified via SMS and can manage the agent with simple commands.

## Stack

- **Rust** + Axum (async web framework)
- **Ollama** for local AI inference (also supports Groq/OpenAI-compatible backends)
- **Twilio** for SMS (BYOA — bring your own account)
- **SQLite** for storage
- **.ics** calendar invites sent via SMS

## Quick Start

```bash
# Clone and build
git clone https://github.com/Ivapo/phonebook.git
cd phonebook
cargo build --release

# Set environment variables
export OLLAMA_URL=http://localhost:11434
export TWILIO_ACCOUNT_SID=your_sid
export TWILIO_AUTH_TOKEN=your_token
export TWILIO_PHONE_NUMBER=+1234567890
export OWNER_PHONE=+1234567890

# Run
cargo run
```

The server starts on port 3000 by default. Point your Twilio webhook to `https://your-domain/webhook/sms`.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `3000` | Server port |
| `DATABASE_URL` | `phonebook.db` | SQLite database path |
| `ADMIN_TOKEN` | `changeme` | Token for admin UI authentication |
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint |
| `TWILIO_ACCOUNT_SID` | | Your Twilio account SID |
| `TWILIO_AUTH_TOKEN` | | Your Twilio auth token |
| `TWILIO_PHONE_NUMBER` | | Your Twilio phone number |
| `OWNER_PHONE` | | Your personal phone number (for notifications and admin commands) |

## How It Works

1. Customer texts your Twilio number
2. The webhook receives the message, checks rate limits and blocklist
3. AI extracts intent (book, cancel, reschedule, confirm, etc.)
4. Conversation engine manages the multi-turn flow, collecting info as needed
5. On confirmation, the booking is saved and you're notified via SMS
6. Reply is sent back to the customer via Twilio REST API

## Admin Commands

Text these from your owner phone number:

| Command | Description |
|---|---|
| `#pause` | Pause the agent (stop replying to customers) |
| `#resume` | Resume the agent |
| `#status` | Show agent status, message count, blocked numbers |
| `#block +1555123456` | Block a phone number |
| `#unblock +1555123456` | Unblock a phone number |

## Rate Limiting

- **Per-customer**: 15 messages/hour per phone number. Exceeding auto-blocks the number.
- **Global**: 100 messages/hour per Twilio number. Exceeding pauses the agent.
- Blocked and rate-limited numbers receive no reply (saves outbound costs).

## Pricing

- **Self-hosted**: $39 one-time
- **Cloud-hosted**: $7/month (we run the server)
- Twilio costs are paid directly by you (~$1/mo for a number + per-message fees)

## Development

```bash
cargo build          # Build
cargo run            # Run dev server
cargo test           # Run tests
cargo clippy         # Lint
cargo fmt            # Format
```

## License

[MIT](LICENSE)
