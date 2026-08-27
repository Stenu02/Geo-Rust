# Documento dei requisiti

Sistema di geolocalizzazione e comunicazione per una flotta di veicoli, in
architettura client/server.


**Convenzione:** `RF` requisito funzionale, `RNF` requisito non funzionale.

---

## 1. Requisiti funzionali

Descrivono **cosa** il sistema deve fare.

| ID | Requisito | Citazione dalla traccia |
|---|---|---|
| **RF1** | Un utente accede al sistema attraverso una fase di registrazione, nella quale definisce un account e una password. | *"Ogni utente accede al sistema attraverso una fase di registrazione, nella quale vengono definiti un account e una password"* |
| **RF2** | Un utente registrato deve potersi autenticare. | *"Il sistema deve permettere a più utenti di registrarsi, autenticarsi"* |
| **RF3** | Le coordinate geografiche di ogni utente sono gestite **in modo emulato**. | *"l'utente può essere monitorato dal server attraverso coordinate geografiche, gestite in modo emulato"* |
| **RF4** | La posizione di ciascun utente è trasmessa al server **ogni 30 secondi**. | *"La posizione di ciascun utente viene trasmessa al server ogni 30 secondi, così da consentire il tracciamento continuo degli spostamenti"* |
| **RF5** | Il sistema gestisce lo stato di ogni utente fra tre valori: **sconnesso**, **fermo**, **in movimento**. | *"un utente può risultare sconnesso, fermo oppure in movimento"* |
| **RF6** | La transizione da *fermo* a *in movimento* avviene quando viene rilevato un cambiamento delle coordinate. | *"La transizione dallo stato «fermo» allo stato «in movimento» avviene quando viene rilevato un cambiamento delle coordinate"* — le slide precisano: *"al primo cambiamento di coordinata"* |
| **RF7** | La transizione da *in movimento* a *fermo* avviene quando, per almeno **tre minuti**, la posizione non cambia. | *"il passaggio dallo stato «in movimento» allo stato «fermo» si verifica quando, per almeno tre minuti, la posizione dell'utente non cambia"* |
| **RF8** | Il server calcola il **tragitto percorso** da uno specifico utente. | *"Il server deve essere in grado di analizzare il movimento compiuto da uno specifico utente. Le informazioni richieste riguardano il tragitto percorso"* |
| **RF9** | Il server calcola la **velocità media** di uno specifico utente. | *"la velocità media"* |
| **RF10** | Il server calcola la **durata complessiva del movimento** e la **durata delle pause**. | *"la durata complessiva del movimento e la durata delle pause"* |
| **RF11** | Le analisi RF8–RF10 devono poter essere calcolate su intervalli temporali programmabili: **giorno corrente**, **settimana corrente**, **mese corrente**. | *"Tali dati devono poter essere calcolati su intervalli temporali programmabili, come il giorno corrente, la settimana corrente o il mese corrente"* |
| **RF12** | Il server invia messaggi di testo **in broadcast**, cioè a tutti gli utenti **connessi**. | *"Il server deve poter inviare messaggi di testo sia in broadcast, cioè a tutti gli utenti connessi"* |
| **RF13** | Il server invia messaggi di testo **diretti a un singolo utente**. | *"sia in modo diretto a un singolo utente"* |
| **RF14** | Ogni utente deve poter inviare messaggi testuali al server. | *"Allo stesso tempo, ogni utente deve poter inviare messaggi testuali al server"* |

---

## 2. Requisiti non funzionali




| ID | Requisito | Citazione dalla traccia |
|---|---|---|
| **RNF1** | L'applicazione deve essere **eseguibile su almeno due piattaforme** fra Windows, Linux, macOS, Android, ChromeOS, iOS. | *"l'applicazione deve essere eseguibile su almeno due piattaforme diverse tra Windows, Linux, macOS, Android, ChromeOS e iOS"* |
| **RNF2** | Il server genera un **file di log** che riporta, **ogni due minuti**, i dettagli relativi al tempo di CPU utilizzato. | *"Il server deve generare un file di log che riporti, ogni due minuti, i dettagli relativi al tempo di CPU utilizzato"* |
| **RNF3** | Attenzione alle prestazioni, in particolare al **consumo di tempo CPU** e alla **dimensione dell'applicativo**. | *"È inoltre richiesto di prestare attenzione alle prestazioni del sistema, in particolare al consumo di tempo CPU e alla dimensione dell'applicativo"* |
| **RNF4** | La **dimensione del file eseguibile** prodotto deve essere indicata nel report descrittivo. | *"nel report descrittivo del progetto deve essere indicata anche la dimensione del file eseguibile prodotto"* |
| **RNF5** | L'applicazione è sviluppata in **Rust**, con architettura **client/server**. | *"un'applicazione client/server sviluppata in Rust"* |

---



