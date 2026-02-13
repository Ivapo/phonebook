CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    business_name TEXT NOT NULL,
    owner_name TEXT NOT NULL,
    owner_phone TEXT NOT NULL,
    twilio_account_sid TEXT NOT NULL DEFAULT '',
    twilio_auth_token TEXT NOT NULL DEFAULT '',
    twilio_phone_number TEXT NOT NULL DEFAULT '',
    availability TEXT,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS bookings (
    id TEXT PRIMARY KEY,
    customer_phone TEXT NOT NULL,
    customer_name TEXT,
    date_time TEXT NOT NULL,
    duration_minutes INTEGER NOT NULL DEFAULT 60,
    status TEXT NOT NULL DEFAULT 'pending',
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS conversations (
    phone TEXT PRIMARY KEY,
    messages TEXT NOT NULL DEFAULT '[]',
    state TEXT NOT NULL DEFAULT 'idle',
    last_activity TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL DEFAULT (datetime('now', '+30 minutes'))
);

CREATE TABLE IF NOT EXISTS blocked_numbers (
    phone TEXT PRIMARY KEY,
    reason TEXT,
    is_auto INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS rate_limits (
    phone_number TEXT NOT NULL,
    message_count INTEGER NOT NULL DEFAULT 1,
    window_start TEXT NOT NULL,
    PRIMARY KEY (phone_number, window_start)
);

CREATE INDEX IF NOT EXISTS idx_bookings_customer_phone ON bookings(customer_phone);
CREATE INDEX IF NOT EXISTS idx_bookings_date_time ON bookings(date_time);
CREATE INDEX IF NOT EXISTS idx_bookings_status ON bookings(status);
CREATE INDEX IF NOT EXISTS idx_rate_limits_window ON rate_limits(window_start);
