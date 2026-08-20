//! Una sessione per connessione. Il cuore asincrono del server.
//!
//! Struttura tipica del loop:
//!
//!   tokio::select! {
//!       frame = framed.next()      => { /* messaggio dal client */ }
//!       msg   = mailbox.recv()     => { /* messaggio diretto (R13) */ }
//!       msg   = broadcast.recv()   => { /* messaggio broadcast (R13) */ }
//!       _     = heartbeat.tick()   => { /* timeout / keepalive */ }
//!       _     = shutdown.cancelled() => break,
//!   }
//!
//! ATTACCO: P2, fasi F2 e F4.

pub struct Session;
