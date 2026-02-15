CREATE TABLE IF NOT EXISTS monthly_activity (
    month TEXT NOT NULL PRIMARY KEY,
    messages_received INTEGER NOT NULL DEFAULT 0,
    messages_sent INTEGER NOT NULL DEFAULT 0,
    bookings_created INTEGER NOT NULL DEFAULT 0,
    bookings_cancelled INTEGER NOT NULL DEFAULT 0,
    bookings_rescheduled INTEGER NOT NULL DEFAULT 0
);
