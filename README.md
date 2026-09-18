# GeoRuggine - G26
### Progetto di Programmazione in Rust, A.A. 2025-2026

Sistema client/server in Rust per seguire una flotta di veicoli: 
- ogni veicolo manda la propria posizione a un server ogni trenta secondi
- il server ricostruisce gli spostamenti, li conserva e li rende interrogabili
- chi coordina la flotta osserva lo stato dei veicoli, chiede resoconti e scambia messaggi con gli autisti

### Membri del gruppo

- Demo Alessandro s361908
- Fontana Francesco s360145
- Nuzzo Stefano s363651
- Voghera Leonardo s354375

---

## Come è organizzato il repository

```
crates/
  geo-core/       le regole: stati, intervalli, calcoli sul movimento
  geo-protocol/   i messaggi che viaggiano sulla rete e la loro codifica
  geo-net/        il lato client della connessione (accesso, riconnessione)
  geo-server/     il server: sessioni, database, valutatore degli stati, log CPU
  geo-client/     il programma dell'autista
  geo-admin/      la console del coordinatore
assets/tracks/    tragitti di esempio per l'emulazione del movimento
docs/             documentazione (vedi sotto)
```

I tre programmi eseguibili sono `geo-server`, `geo-client` e `geo-admin`, gli
altri tre crate sono librerie condivise.

---

## Documentazione

| Documento | Per chi |
|---|---|
| [`docs/manuale_utente.md`](docs/manuale_utente.md) | chi deve avviare e usare i programmi |
| [`docs/manuale_sviluppatore.md`](docs/manuale_sviluppatore.md) | chi deve leggere o modificare il codice |
| [`docs/report_descrittivo.md`](docs/report_descrittivo.md) | tracciabilità dei requisiti, prestazioni, portabilità |
| [`docs/requisiti.md`](docs/requisiti.md) | i requisiti estratti dalla traccia |
| [`docs/database.md`](docs/database.md) | lo schema del database |

---

## Avvio rapido

Serve Rust (la versione è fissata in `rust-toolchain.toml`).

```bash
cargo build --release
./target/release/geo-server --create-admin capoflotta    # una volta sola
./target/release/geo-server                              # terminale 1
./target/release/geo-client --username camion01 --register --track assets/tracks/con_pause.csv   # terminale 2
./target/release/geo-admin --username capoflotta         # terminale 3
```

Su Windows gli eseguibili hanno estensione `.exe`. La sequenza completa, con
le opzioni di ogni programma, è nel manuale utente.