-- Schema del database.


PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Utenti --------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    -- Stringa PHC completa di Argon2 (contiene salt e parametri). Mai la password.
    password_hash TEXT    NOT NULL,
    -- 'trucker' | 'admin'
    role          TEXT    NOT NULL DEFAULT 'trucker',
    created_at    TEXT    NOT NULL
);

-- Posizioni ricevute dai camionisti -----------------------------------------
CREATE TABLE IF NOT EXISTS positions (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- ISO 8601 UTC. Le conversioni al fuso locale avvengono solo in geo-core.
    at       TEXT    NOT NULL,
    lat      REAL    NOT NULL,
    lon      REAL    NOT NULL
);

-- Indice essenziale: ogni interrogazione filtra per utente e periodo.
CREATE INDEX IF NOT EXISTS idx_positions_user_at ON positions(user_id, at);

-- Intervalli di stato -------------------------------------------------------
-- Intervalli e non singoli eventi: rende dirette le somme delle durate e banale
-- il troncamento di un intervallo sui confini del periodo analizzato.
-- end_at NULL = intervallo ancora aperto (lo stato corrente dell'utente).
CREATE TABLE IF NOT EXISTS state_intervals (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 'disconnected' | 'stopped' | 'moving'
    state    TEXT    NOT NULL,
    start_at TEXT    NOT NULL,
    end_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_intervals_user_start ON state_intervals(user_id, start_at);

-- Al piu' un intervallo aperto per utente. Invariante forte: se questo indice
-- viene violato, avete un bug nella chiusura degli intervalli e ve ne accorgete
-- subito invece che scoprendo numeri sbagliati in demo.
CREATE UNIQUE INDEX IF NOT EXISTS idx_intervals_one_open
    ON state_intervals(user_id) WHERE end_at IS NULL;

-- Messaggi di chat ----------------------------------------------------------
CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    -- NULL = dall'amministratore verso i camionisti
    from_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    -- NULL = broadcast
    to_user_id   INTEGER REFERENCES users(id) ON DELETE CASCADE,
    body         TEXT    NOT NULL,
    sent_at      TEXT    NOT NULL,
    -- Per la coda offline: i messaggi diretti non consegnati partono a 0
    delivered    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_messages_pending
    ON messages(to_user_id, delivered) WHERE delivered = 0;
