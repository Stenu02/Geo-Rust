# Manuale dello sviluppatore

GeoRuggine segue una flotta di veicoli. Ogni veicolo manda la propria posizione
a un server ogni trenta secondi, il server capisce se il veicolo è fermo o in
movimento, tiene lo storico degli spostamenti e permette a chi coordina la
flotta di consultarlo e di scambiare messaggi con gli autisti.

Questo documento spiega com'è fatto il programma dentro.

---



## 1. I tre programmi

Dalla compilazione escono tre eseguibili.

| Programma | Chi lo usa | Cosa fa |
|---|---|---|
| `geo-server` | chi gestisce il sistema | ascolta sulla rete, decide gli stati, salva tutto, smista i messaggi |
| `geo-client` | l'autista | manda la posizione ogni trenta secondi e scambia messaggi con la centrale |
| `geo-admin` | il coordinatore | mostra la flotta, chiede i resoconti, scrive agli autisti |

Il server sta al centro: i due programmi degli utenti non si parlano mai fra
loro, passano sempre da lui.

---

## 2. Come è diviso il codice

Il progetto è un workspace Cargo con sei crate.

| Crate | Cosa contiene | Costruito sopra |
|---|---|---|
| `geo-core` | le regole: posizioni, stati, calcoli, periodi | niente |
| `geo-protocol` | i messaggi che viaggiano sulla rete | `geo-core` |
| `geo-net` | il lato client della connessione | `geo-protocol` |
| `geo-server` | ascolto, sessioni, database, analisi | `geo-protocol` |
| `geo-client` | il programma dell'autista | `geo-net` |
| `geo-admin` | la console del coordinatore | `geo-net` |

Le frecce delle dipendenze vanno tutte in una direzione: da chi fa il lavoro
concreto verso chi definisce le regole. Nessun crate dipende da chi gli sta
sopra.

```
        geo-client        geo-admin        geo-server
             \                /                 |
              \              /                  |
               \--- geo-net /                   |
                      |                         |
                      +------ geo-protocol -----+
                                    |
                                geo-core
```



---

## 3. Il giro di un dato

Tre percorsi spiegano quasi tutto il funzionamento.

### Una posizione che arriva

```
  geo-client                geo-server
  ----------                ----------
  simulator                 session      riceve il messaggio
     |                         |
  main.rs  --- rete --->    registry     aggiorna la macchina a stati
                               |
                            storage      scrive nel database
```

Il client calcola la posizione con la strategia scelta e la manda. Sul server la
riceve `session`, che la passa a due destinatari diversi: al `registry`, che
tiene in memoria lo stato di chi è collegato e decide se questa posizione è un
movimento, e allo `storage`, che la scrive su disco.

Se lo stato cambia, il `registry` restituisce la transizione e `session` la fa
salvare anch'essa.

### Un resoconto richiesto dal coordinatore

```
  geo-admin  --- rete --->  session  --->  storage    legge dal database
                               |
                               v
                            admin.rs  --->  analytics  (in geo-core)
                               |
  geo-admin  <--- rete ---     +            resoconto già calcolato
```

Il conto lo fa il server, non la console. La console riceve un resoconto finito
e si limita a mostrarlo. È una scelta voluta: i dati stanno sul server, e
mandarli tutti alla console per farglieli sommare sarebbe più traffico e più
codice duplicato.

### Il passaggio a "fermo"

Questo non lo scatena nessun messaggio, ed è il punto meno ovvio del programma.

Un veicolo passa da *in movimento* a *fermo* quando le coordinate non cambiano
per tre minuti. Ma un autista che si ferma davvero, o che spegne il client, non
manda niente: se aspettassimo un messaggio per accorgercene, resterebbe *in
movimento* per sempre.

Perciò c'è un task a parte, `evaluator`, che ogni dieci secondi chiede al
`registry` di ricontrollare tutti i collegati e vedere a chi è scaduta la
soglia. Il controllo periodico decide solo *quando ce ne accorgiamo*: la
transizione porta comunque la data in cui la soglia è scaduta davvero.

---

## 4. I file

### `geo-core` - le regole

Qui non succede niente di concreto: si calcola soltanto. Se devi cambiare *cosa
significa* essere fermi, o come si misura un tragitto, il file è in questa
cartella.

| File | Contiene |
|---|---|
| `lib.rs` | i valori condivisi: i 30 secondi fra due invii, i 3 minuti della soglia, la tolleranza sotto cui due coordinate sono la stessa, il fuso orario |
| `position.rs` | la coppia latitudine/longitudine, il confronto fra due posizioni, il controllo di validità |
| `geo_math.rs` | la distanza in chilometri fra due coordinate, tenendo conto della curvatura terrestre |
| `state_machine.rs` | i tre stati e i quattro passaggi: accesso, nuova coordinata, tempo scaduto, disconnessione |
| `interval.rs` | un periodo passato in un certo stato, con inizio e fine |
| `timerange.rs` | traduce «giorno corrente», «settimana corrente», «mese corrente» in due date precise |
| `analytics.rs` | da intervalli e posizioni ricava il resoconto: chilometri, velocità medie, tempi di marcia e di sosta |

### `geo-protocol` - cosa viaggia sulla rete

| File | Contiene |
|---|---|
| `messages.rs` | i messaggi, divisi in quattro gruppi secondo chi parla e chi ascolta, più i codici di errore e il numero di versione |
| `codec.rs` | come un messaggio diventa una sequenza di byte e come i byte si riseparano in messaggi |

### `geo-net` - il lato client della connessione

| File | Contiene |
|---|---|
| `connection.rs` | apre la connessione, si presenta, si autentica, tiene vivo il collegamento e lo riapre quando cade |
| `events.rs` | gli avvisi che manda a chi la usa: collegato, entrato, messaggio ricevuto, caduto, errore |

Chi riceve un avviso non sa niente di rete: vede solo che è successo qualcosa.

### `geo-server` - il cuore

| File | Contiene |
|---|---|
| `main.rs` | legge gli argomenti, avvia, gestisce Ctrl-C, crea l'account del coordinatore |
| `lib.rs` | apre la porta, accetta le connessioni, avvia i due lavori periodici |
| `session.rs` | la conversazione con un singolo collegato: qui si decide cosa fare a ogni messaggio |
| `registry.rs` | chi è collegato adesso, con il suo stato. Vive in memoria e sparisce col processo |
| `storage.rs` | l'unico che tocca il database: tutti gli altri gli mandano richieste |
| `schema.sql` | le quattro tabelle, incorporate nell'eseguibile e create al primo avvio |
| `auth.rs` | calcolo e verifica delle password, regole su cosa è un account accettabile |
| `admin.rs` | i resoconti: prende intervalli e posizioni dal database e li passa ai calcoli di `geo-core` |
| `evaluator.rs` | il controllo periodico che fa scattare il passaggio a fermo |
| `cpu_log.rs` | legge dal sistema il tempo di CPU consumato dal processo, con una versione per Unix e una per Windows, e ricava il consumo degli ultimi due minuti e la media mobile |
| `tests/integrazione.rs` | avvia un server vero e gli parla con client veri |



### `geo-client` - il programma dell'autista

| File | Contiene |
|---|---|
| `main.rs` | argomenti, scelta della strategia, il ciclo che manda una posizione ogni trenta secondi |
| `simulator.rs` | le quattro strategie di movimento e la lettura del file traccia |
| `tui.rs` | lo schermo a tre riquadri: stato, conversazione, riga di scrittura |

Con `--headless` il client gira senza interfaccia e si limita a mandare le
posizioni: serve per avviarne molti da script, con la password presa dalla
variabile d'ambiente `GEO_PASS`.

### `geo-admin` - la console del coordinatore

| File | Contiene |
|---|---|
| `main.rs` | argomenti e accesso |
| `network.rs` | il filo con il server: manda i comandi, riceve le risposte, richiede la flotta a intervalli |
| `tui.rs` | le due schede, la tabella della flotta, la chat, i comandi testuali e i tasti per cambiare periodo del resoconto |

---



## 5. Il database

Un file SQLite, quattro tabelle. Lo schema sta in `schema.sql` ed è incorporato
nell'eseguibile, quindi al primo avvio le tabelle si creano da sole.

| Tabella | Contiene | Cresce |
|---|---|---|
| `users` | account e impronte delle password | a ogni registrazione |
| `positions` | lo storico degli spostamenti | due righe al minuto per veicolo |
| `state_intervals` | i periodi passati in ciascuno stato | a ogni cambio di stato |
| `messages` | lo storico della chat | a ogni messaggio |

`state_intervals` è la tabella che rende possibili i resoconti: invece di
ricalcolare gli stati dalle posizioni ogni volta, il server li registra man mano
che avvengono. Un intervallo ancora in corso ha la fine vuota.

Le password non vengono mai salvate. Si conserva un'impronta calcolata con
Argon2, con un pezzo casuale diverso per ogni utente, dalla quale non si può
risalire all'originale.

Per il dettaglio delle colonne e dei vincoli c'è `docs/database.md`.

---


## Documenti collegati

`docs/requisiti.md` elenca cosa chiede la traccia. `docs/database.md` descrive
le quattro tabelle nel dettaglio. Il manuale utente spiega come si usano i tre
programmi.