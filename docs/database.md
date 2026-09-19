# Il database

Il server conserva i propri dati in SQLite, in un unico file il cui percorso
si indica con `--db` (predefinito: `georuggine.db`).





## Quadro d'insieme

```
                    users
                      │
       ┌──────────────┼──────────────┬──────────────┐
       │              │              │              │
   positions   state_intervals   messages       messages
                                 (from_user)    (to_user)
```

Quattro tabelle. `users` è l'unica indipendente, le altre tre vi fanno
riferimento.

| Tabella | Contiene | Cresce |
|---|---|---|
| `users` | account e credenziali | con le registrazioni |
| `positions` | storico degli spostamenti | ~2 righe al minuto per veicolo |
| `state_intervals` | storico degli stati | a ogni cambio di stato |
| `messages` | storico della chat | a ogni messaggio |

---

## `users` - account e credenziali

```sql
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    role          TEXT    NOT NULL DEFAULT 'trucker',
    created_at    TEXT    NOT NULL
);
```

| Colonna | Significato |
|---|---|
| `id` | Identificatore assegnato da SQLite. È il valore che viaggia nel protocollo. |
| `username` | Nome scelto dall'utente. Il vincolo `UNIQUE` è ciò che rifiuta i duplicati. |
| `password_hash` | Stringa PHC completa di Argon2. Mai la password in chiaro. |
| `role` | `'trucker'` oppure `'admin'`. Distingue le due entità del sistema. |
| `created_at` | Istante di creazione. |

---

## `positions` - lo storico degli spostamenti

```sql
CREATE TABLE IF NOT EXISTS positions (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    at       TEXT    NOT NULL,
    lat      REAL    NOT NULL,
    lon      REAL    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_positions_user_at ON positions(user_id, at);
```

| Colonna | Significato |
|---|---|
| `user_id` | A chi appartiene. `ON DELETE CASCADE`: cancellando un utente si cancella il suo storico. |
| `at` | Istante della rilevazione. |
| `lat`, `lon` | Gradi decimali. Latitudine prima, longitudine dopo. |




### L'indice

Ogni interrogazione filtra per utente e per intervallo di tempo. L'indice
composto `(user_id, at)` copre esattamente quel modo di leggere, ed è ciò che
evita una scansione completa quando la tabella cresce.

---

## `state_intervals` - lo storico degli stati

```sql
CREATE TABLE IF NOT EXISTS state_intervals (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    state    TEXT    NOT NULL,
    start_at TEXT    NOT NULL,
    end_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_intervals_user_start ON state_intervals(user_id, start_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_intervals_one_open
    ON state_intervals(user_id) WHERE end_at IS NULL;
```

| Colonna | Significato |
|---|---|
| `state` | `'disconnected'`, `'stopped'` oppure `'moving'`. |
| `start_at` | Inizio dell'intervallo, ISO 8601 UTC. |
| `end_at` | Fine. `NULL` significa "ancora in corso", cioè lo stato attuale dell'utente. |

### I due indici

Il primo, su `(user_id, start_at)`, serve alle interrogazioni: si legge sempre
per utente e per periodo. Il secondo è un vincolo: è unico e vale solo sulle
righe con `end_at` vuoto, quindi per ogni utente può esistere al massimo un
intervallo aperto. Un utente è in uno stato solo alla volta, e a garantirlo è
il database, non un controllo nel codice: un secondo intervallo aperto
verrebbe rifiutato dalla scrittura.





### Gli intervalli pendenti dopo un arresto anomalo

Se il server termina in modo anomalo mentre alcuni utenti sono collegati, i loro
intervalli restano aperti: nessuno tornerà a chiuderli.

Il problema non è l'errore in sé, ma che cresce: la durata di un intervallo
aperto si calcola fino a "adesso", quindi un utente scollegato da tre giorni
risulterebbe fermo da tre giorni, e il valore aumenterebbe a ogni interrogazione.

All'avvio, prima di accettare connessioni, il server chiude tutti gli
intervalli rimasti aperti usando l'istante di avvio come fine. È una scelta
conservativa: non si può sapere quando il server sia morto davvero, ma un errore
limitato è preferibile a uno che cresce senza fine.

L'ordine conta: farlo dopo aver accettato connessioni chiuderebbe anche gli
intervalli appena aperti da chi si sta ricollegando.

---

## `messages` - lo storico della chat

```sql
CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    from_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    to_user_id   INTEGER REFERENCES users(id) ON DELETE CASCADE,
    body         TEXT    NOT NULL,
    sent_at      TEXT    NOT NULL,
    delivered    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_messages_pending
    ON messages(to_user_id, delivered) WHERE delivered = 0;
```

### I due `NULL` hanno significato

| Campo | `NULL` significa |
|---|---|
| `from_user_id` | il messaggio viene dall'amministratore |
| `to_user_id` | il messaggio è un broadcast, diretto a tutti |

Le quattro combinazioni coprono l'intero spazio dei messaggi previsti:

| `from` | `to` | Tipo |
|---|---|---|
| `NULL` | `NULL` | annuncio dell'amministratore a tutta la flotta |
| `NULL` | id | messaggio diretto dell'amministratore a un camionista |
| id | `NULL` | messaggio di un camionista verso la console |
| id | id | non usato attualmente |