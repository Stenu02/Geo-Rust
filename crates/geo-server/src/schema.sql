-- Schema del database GeoRuggine (R15).
--
-- Incorporato nel binario con include_str!: nessun file da distribuire accanto
-- all'eseguibile, che sarebbe il modo piu' banale di rompere R16.
--
-- Va scritto COMPLETO nella fase comune: cambiare lo schema dopo significa
-- toccare storage.rs, le query delle analisi e lo script di seeding insieme.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- R1, R2, R12 --------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    -- Stringa PHC completa di Argon2 (contiene salt e parametri). Mai la password.
    password_hash TEXT    NOT NULL,
    -- 'trucker' | 'admin'
    role          TEXT    NOT NULL DEFAULT 'trucker',
    created_at    TEXT    NOT NULL
);

-- R4, R8 -------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS positions (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- ISO 8601 UTC. Le conversioni al fuso locale avvengono solo in geo-core.
    at       TEXT    NOT NULL,
    lat      REAL    NOT NULL,
    lon      REAL    NOT NULL
);

-- Indice essenziale: ogni interrogazione di R11 filtra per utente e periodo.
CREATE INDEX IF NOT EXISTS idx_positions_user_at ON positions(user_id, at);

-- R5, R10 ------------------------------------------------------------------
-- Intervalli, non eventi: rende dirette le somme di R10 e banale il troncamento di R11.
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

-- R13, R14 -----------------------------------------------------------------
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
