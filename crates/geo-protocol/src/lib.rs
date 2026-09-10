//! Il contratto di rete di GeoRuggine: quali byte viaggiano sul filo.
//!
//! Separato da `geo-core` perche' cosi' potete cambiare il formato wire
//! (es. MessagePack -> JSON per misurare la differenza di dimensione) senza
//! toccare il codice della macchina a stati.

pub mod codec;
pub mod messages;

pub use codec::{decode, encode, frame_codec, CodecError};
pub use messages::{
    AdminMessage, AdminReply, ClientMessage, ErrorCode, FleetEntry, Hello, Role, ServerMessage,
    PROTOCOL_VERSION,
};
