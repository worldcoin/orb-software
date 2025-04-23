-- Add a trivial table so that `sqlx::migrate!()` has something to manage.

CREATE TABLE IF NOT EXISTS greetings (
    id SERIAL PRIMARY KEY,
    message TEXT NOT NULL
);
