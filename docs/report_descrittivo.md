# GeoRuggine - Report descrittivo

Questo documento descrive cosa è stato realizzato, come è organizzato e con
quali risultati rispetto ai requisiti della traccia. 

---

## 1. Cos'è il progetto

GeoRuggine è un sistema client/server per seguire una flotta di veicoli. Ogni
veicolo manda la propria posizione al server ogni trenta secondi; il server
ricostruisce gli spostamenti, li conserva in un database e li rende
interrogabili; chi coordina la flotta osserva lo stato di ciascun veicolo, chiede
resoconti sul movimento e scambia messaggi con gli autisti.

Sono tre programmi da terminale:

| Programma | Chi lo usa | Cosa fa |
|---|---|---|
| `geo-server` | chi gestisce il sistema | accetta le connessioni, decide gli stati, salva tutto, smista i messaggi |
| `geo-client` | l'autista | emula il movimento, manda la posizione, chatta con la centrale |
| `geo-admin` | il coordinatore | mostra la flotta in tempo reale, chiede i resoconti, scrive agli autisti |

Le posizioni sono emulate: il client può seguire un tragitto letto da file,
muoversi in linea retta fra due punti, generare posizioni casuali entro un
raggio, o restare fermo e farsi spostare a mano.

---

## 2. Architettura

Il codice è un workspace Cargo di sei crate. Tre sono librerie condivise, tre
sono i programmi.

| Crate | Ruolo | Dipende da                            |
|---|---|---------------------------------------|
| `geo-core` | le regole: stati e transizioni, intervalli di tempo, calcoli sul movimento, validità delle coordinate | -                                     |
| `geo-protocol` | i messaggi che viaggiano sulla rete e la loro codifica | `geo-core`                            |
| `geo-net` | il lato client della connessione: handshake, accesso, riconnessione automatica | `geo-core`, `geo-protocol`            |
| `geo-server` | il server: sessioni, registro dei collegati, database, valutatore degli stati, log della CPU | `geo-core`, `geo-protocol`            |
| `geo-client` | il programma dell'autista: emulazione del movimento e interfaccia a terminale | `geo-core`, `geo-protocol`, `geo-net` |
| `geo-admin` | la console del coordinatore | `geo-core`, `geo-protocol`, `geo-net` |

La separazione ha uno scopo preciso: le regole del sistema (quando un veicolo è
fermo, come si calcola un tragitto) stanno in `geo-core`, che non sa nulla di
rete né di database; il server le applica, i client le riusano solo per
leggere.

Il server e i due client comunicano su TCP. Ogni messaggio è preceduto dalla
propria lunghezza ed è codificato in MessagePack, un formato binario compatto
che serde serializza direttamente dalle strutture Rust. Il primo scambio di
ogni connessione dichiara versione del protocollo e ruolo: una versione diversa
viene rifiutata con un errore esplicito.

Dentro il server ogni connessione vive in un task asincrono indipendente
(runtime Tokio); un solo task possiede il database SQLite e gli altri gli
inviano richieste su un canale; un registro in memoria tiene chi è collegato in
quel momento con il suo stato. I dettagli sono nel manuale dello sviluppatore,
sezione 3.

---

## 3. Tracciabilità dei requisiti

Numerazione e testo dei requisiti sono in `docs/requisiti.md`. Per ognuno,
dove è realizzato nel codice.

### Requisiti funzionali

| ID | Requisito | Dove |
|---|---|---|
| RF1 | Registrazione con account e password | `geo-server/auth.rs` (regole e impronta Argon2), `session.rs`, tabella `users`; `geo-client --register` |
| RF2 | Autenticazione degli utenti registrati | `session.rs` (`Login` per gli autisti, `AdminMessage::Login` per il coordinatore), `auth.rs` |
| RF3 | Coordinate gestite in modo emulato | `geo-client/simulator.rs`: file traccia, interpolazione fra due punti con pause, generatore casuale, inserimento manuale (`/pos`) |
| RF4 | Posizione trasmessa ogni 30 secondi | `geo-client/main.rs` (intervallo di 30 s), `geo-server/session.rs` (`PositionUpdate`), tabella `positions` |
| RF5 | Stati sconnesso, fermo, in movimento | `geo-core/state_machine.rs`; `geo-server/registry.rs` tiene lo stato di ogni collegato; tabella `state_intervals` per lo storico |
| RF6 | Da fermo a in movimento al primo cambio di coordinata | `state_machine.rs::on_position` |
| RF7 | Da in movimento a fermo dopo tre minuti senza cambiamenti | `state_machine.rs::on_tick`; `geo-server/evaluator.rs` lo chiama ogni 10 s per tutti i collegati, con l'istante reale di scadenza |
| RF8 | Tragitto percorso | `geo-core/analytics.rs::total_distance_km`, distanza fra punti in `geo_math.rs` |
| RF9 | Velocità media | `analytics.rs::avg_speed`, calcolata sul tempo in movimento e sull'intero periodo |
| RF10 | Durata del movimento e delle pause | `analytics.rs::total_in` sugli intervalli di stato, con ritaglio sul periodo in `interval.rs` |
| RF11 | Periodi programmabili: giorno, settimana, mese correnti | `geo-core/timerange.rs`; nella console `/report oggi\|settimana\|mese` e i tasti `1 2 3` |
| RF12 | Messaggi in broadcast a tutti i connessi | `session.rs` (`SendBroadcast`), `registry.rs::annuncia`; nella console `/all` |
| RF13 | Messaggi diretti a un singolo utente | `session.rs` (`SendDirect`), `registry.rs::send_to`; se il destinatario è assente, `storage.rs::take_pending` lo consegna al collegamento successivo |
| RF14 | Messaggi dall'utente al server | `session.rs` (`Chat`), inoltro alla console tramite `registry.rs::verso_admin`, salvataggio in `messages` |

### Requisiti non funzionali

| ID | Requisito | Dove |
|---|---|---|
| RNF1 | Eseguibile su almeno due piattaforme | tutto il codice è portabile tranne la lettura del tempo di CPU, in `geo-server/cpu_log.rs` con due versioni (`unix`, `windows`); sezione 5 |
| RNF2 | Log del tempo di CPU ogni due minuti | `geo-server/lib.rs` (task periodico a 120 s), `cpu_log.rs`; sezione 4.2 |
| RNF3 | Attenzione a consumo di CPU e dimensione | profilo di compilazione e scelte di progetto; sezioni 4.1 e 4.2 |
| RNF4 | Dimensione dell'eseguibile nel report | sezione 4.1 |
| RNF5 | Rust, architettura client/server | workspace Cargo, tre processi distinti |

---

## 4. Prestazioni

### 4.1 Dimensione degli eseguibili

Compilazione in release con Rust 1.93.1.

| Eseguibile | macOS (arm64) | Windows (x86-64) | macOS con il profilo standard di Cargo, per confronto |
|---|---|---|---|
| `geo-server` | 2 012 064 byte (1,92 MiB) | 2450432 (2,34 MiB) | 6 064 504 byte (5,78 MiB) |
| `geo-client` | 1 239 872 byte (1,18 MiB) | 1456128 (1,39 MiB) | 3 230 296 byte (3,08 MiB) |
| `geo-admin` | 1 139 392 byte (1,09 MiB) | 1276928 (1,22 MiB) | 2 809 656 byte (2,68 MiB) |

L'ultima colonna riporta gli stessi programmi compilati con il profilo di
release predefinito di Cargo. La differenza è dovuta al profilo scelto nel
`Cargo.toml`: ottimizzazione per la dimensione (`opt-level = "z"`),
ottimizzazione sull'intero programma (`lto`) e rimozione dei simboli di debug
(`strip`).

### 4.2 Il log del tempo di CPU

Il server scrive ogni due minuti una riga nel file indicato con `--log`
(`cpu_usage.csv` se non specificato). Il tempo di CPU viene letto dal sistema
operativo, quindi è il tempo effettivamente consumato dal processo e non il
tempo trascorso: su Unix con `getrusage`, su Windows con `GetProcessTimes`, in
entrambi i casi sommando tempo utente e tempo di sistema.

| Colonna | Significato |
|---|---|
| `timestamp` | data e ora locale del campione |
| `cpu_time_ms` | tempo di CPU totale consumato dall'avvio, in millisecondi |
| `delta_cpu_time_ms` | quanto ne è stato consumato negli ultimi due minuti |
| `rolling_avg_ms` | media dei delta degli ultimi dieci campioni |

Il file viene aperto e chiuso a ogni scrittura, così resta leggibile mentre il
server è in funzione; se esiste già, si continua ad accodare.

---

## 5. Portabilità

Il sistema è stato compilato e provato per intero su due piattaforme:

| Piattaforma | Versione | Architettura |
|---|---|---|
| macOS | 26.6 | arm64 (Apple Silicon) |
| Windows | 11 (10.0.26200) | x86-64 |

Su entrambe: compilazione in release, avvio di server, autista e console con le
tre modalità di movimento, scambio di messaggi nelle due direzioni, resoconti,
chiusura con Ctrl-C, produzione del log della CPU.

L'unico codice che dipende dal sistema operativo è la lettura del tempo di CPU,
perché non esiste una funzione della libreria standard che la fornisca:
`cpu_log.rs` ha due implementazioni, selezionate a tempo di compilazione con
`#[cfg(unix)]` e `#[cfg(windows)]`, e due dipendenze dichiarate solo per la
piattaforma corrispondente (`libc` e `windows-sys`).

---

## Documenti collegati

- `docs/manuale_utente.md` - installazione, avvio, uso dei tre programmi
- `docs/manuale_sviluppatore.md` - struttura del codice, flussi principali, scelte di progetto, database
- `docs/requisiti.md` - testo dei requisiti con le citazioni dalla traccia
- `docs/database.md` - schema del database