# Manuale utente

GeoRuggine tiene sotto controllo una flotta di veicoli. Ogni veicolo manda la
propria posizione al server ogni trenta secondi; il server ricostruisce gli
spostamenti e li conserva, e chi coordina la flotta li consulta e scambia
messaggi con gli autisti.

Sono tre programmi distinti, tutti da terminale.

| Programma | Chi lo usa | A cosa serve |
|---|---|---|
| `geo-server` | chi gestisce il sistema | raccoglie le posizioni, tiene lo storico, smista i messaggi |
| `geo-client` | l'autista | manda la posizione e parla con la centrale |
| `geo-admin` | il coordinatore | guarda la flotta, chiede i resoconti, scrive agli autisti |

In tutti e tre la password viene chiesta a schermo e non compare mentre la
digiti.

---

# Avvio

I passi 1 e 2 si fanno una volta sola. Dal passo 4 in avanti il server deve
essere già avviato. Servono terminali diversi, uno per programma, perché
ciascuno resta aperto.

## 1. Compilare

```bash
cargo build --release
```

Gli eseguibili finiscono in `target/release/`: `geo-server`, `geo-client` e
`geo-admin`.

Su Windows i comandi sono gli stessi, con l'estensione `.exe` e le barre al
contrario: `.\target\release\geo-server.exe`.

## 2. Creare l'account del coordinatore

Non serve che il server sia avviato: il comando scrive nel database e termina.

```bash
./target/release/geo-server --create-admin capoflotta --db georuggine.db
```

```
Password del nuovo amministratore:
Ripeti la password:
amministratore 'capoflotta' creato (id 1).
```

L'account resta nel file del database e vale anche dopo i riavvii. Gli autisti
invece si registrano da soli, al passo 4.

## 3. Avviare il server

```bash
./target/release/geo-server --db georuggine.db --log cpu_usage.csv
```

Va lasciato aperto: tutto passa da lui. Si mette in ascolto sulla porta 7878 di
tutte le interfacce di rete, quindi accetta anche client di altre macchine; con
`--bind 127.0.0.1:7878` resta raggiungibile solo dalla propria. Deve puntare
allo stesso `--db` usato al passo 2, altrimenti non trova l'account del
coordinatore.

Nel file indicato con `--log` scrive ogni due minuti una riga con il tempo di
CPU consumato (il dettaglio delle colonne è negli approfondimenti). Si chiude
con **Ctrl-C**; premendolo una seconda volta esce subito.

## 4. Collegare un autista

In un secondo terminale, con il server già avviato. La prima volta si aggiunge
`--register`:

```bash
./target/release/geo-client --username camion01 --register --track assets/tracks/con_pause.csv
```

```
Password:
Ripeti la password:
```

Dalle volte successive si omette `--register` e la password viene chiesta una
volta sola:

```bash
./target/release/geo-client --username camion01 --track assets/tracks/con_pause.csv
```

Lasciarlo per sbaglio non fa danni: il server risponde che il nome è già preso
e il collegamento prosegue. Con una password sbagliata invece il programma
termina segnalando l'errore.

Se il server gira su un'altra macchina si aggiunge `--server 192.168.1.10:7878`
con il suo indirizzo. Se il collegamento cade, il client riprova da solo, prima
ogni due secondi e poi a intervalli sempre più lunghi fino a un minuto, finché
il server non torna: non serve riavviarlo.

Il movimento è simulato e si sceglie come:

| Opzione | Comportamento |
|---|---|
| `--track percorso.csv` | segue un tragitto registrato in un file |
| `--interpolated "45.06,7.67;45.05,7.71"` | va in linea retta da un punto all'altro |
| `--random "45.07,7.68;5"` | si sposta a caso entro cinque chilometri dal punto |
| nessuna delle tre | resta fermo, e lo si muove a mano |



Lo schermo si divide in tre riquadri: in alto lo stato e le coordinate attuali,
al centro la conversazione con la centrale, in basso la riga per scrivere.

| Come si usa | |
|---|---|
| testo e `Invio` | manda un messaggio alla centrale |
| `/pos 45.07,7.68` | sposta il veicolo a mano |
| `Esc` | esce dal programma |

I messaggi in arrivo compaiono nella conversazione, marcati `[DIRETTO]` o
`[BROADCAST]` a seconda che siano stati mandati a lui solo o a tutta la flotta.

Quando il file traccia finisce, l'autista resta sull'ultima posizione e dopo tre
minuti il server lo segna come fermo.

## 5. Aprire la console del coordinatore

In un terzo terminale, con il nome creato al passo 2:

```bash
./target/release/geo-admin --username capoflotta
```

```
Password:
```

Senza `--username` il nome viene chiesto a schermo insieme alla password. Se le
credenziali sono sbagliate, le richiede.

La prima scheda mostra a sinistra la tabella della flotta, aggiornata da sola
ogni secondo, con stato, ultima posizione e orario dell'ultimo aggiornamento di
ciascun autista; a destra l'elenco degli autisti e la chat con quello
selezionato. La seconda scheda mostra i resoconti. Gli autisti che si collegano
dopo compaiono da soli, e l'ultima riga dello schermo ricorda i comandi
disponibili.

La console ha due modalità: fuori dalla scrittura i tasti sono comandi, dentro
sono testo.

| Fuori dalla scrittura | |
|---|---|
| `T` | entra in modalità scrittura |
| `↑` `↓` | scegli l'autista |
| `Q` | apre la scheda dei resoconti con la giornata dell'autista scelto |
| `1` `2` `3` | nella scheda dei resoconti: giornata, settimana, mese |
| `Tab` · `Shift+Tab` | passa da una scheda all'altra |
| `Esc` | esci dal programma |

| In modalità scrittura | |
|---|---|
| testo e `Invio` | messaggio all'autista selezionato |
| `/all buonasera a tutti` | messaggio a tutta la flotta collegata |
| `/report oggi` · `/report settimana` · `/report mese` | resoconto sul periodo |
| `←` `→` | sposti il cursore nel testo |
| `Esc` | esci dalla scrittura senza inviare |

Il resoconto riporta i chilometri percorsi, la velocità media (calcolata sul
solo tempo in movimento e anche sull'intero periodo), quanto tempo l'autista è
stato in movimento, quanto in sosta e con quante soste, quanto sconnesso.

Un messaggio scritto a un autista non collegato gli viene consegnato al
collegamento successivo. Un messaggio a tutta la flotta raggiunge solo chi è
collegato in quel momento, e nella console compare in tutte le chat.

---

# Approfondimenti

## Account e password

| | Regola |
|---|---|
| Password | almeno 8 caratteri, non più di 128 |
| Nome utente | da 1 a 32 caratteri, senza spazi |

I nomi sono unici: un nome già preso viene rifiutato.

In registrazione la password viene chiesta due volte, in accesso una sola.

Nel database non viene conservata nessuna password, ma solo un'impronta
calcolata con Argon2. Una password dimenticata non è quindi recuperabile: si
crea un nuovo account.



## Gli stati

Ogni autista si trova sempre in uno di tre stati, e a deciderlo è il server.

| Stato | Quando |
|---|---|
| **Sconnesso** | finché non si collega |
| **Fermo** | appena fa l'accesso, e ogni volta che le coordinate non cambiano per tre minuti |
| **In movimento** | alla prima coordinata diversa dalla precedente |

I tre minuti si contano sull'orologio e non sui messaggi ricevuti: se un autista
spegne il client di colpo, il passaggio a fermo avviene lo stesso.

## Il log della CPU

Il file indicato con `--log` è un CSV con una riga ogni due minuti. Se il file
esiste già, il server continua ad accodare.

| Colonna | |
|---|---|
| `timestamp` | data e ora locale del campione |
| `cpu_time_ms` | tempo di CPU consumato dall'avvio, in millisecondi |
| `delta_cpu_time_ms` | quanto ne è stato consumato negli ultimi due minuti |
| `rolling_avg_ms` | media dei delta degli ultimi dieci campioni |


