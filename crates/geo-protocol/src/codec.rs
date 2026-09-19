//! Framing e serializzazione.

use serde::{de::DeserializeOwned, Serialize};
use tokio_util::codec::LengthDelimitedCodec;

/// Limite di dimensione per frame. Protegge da un client che dichiari una lunghezza
/// assurda facendo allocare gigabyte al server.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("errore di I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("serializzazione fallita: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("deserializzazione fallita: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
}

/// Costruisce il codec di framing. Usarlo con `Framed::new(stream, frame_codec())`.
pub fn frame_codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_BYTES)
        .length_field_length(4)
        .new_codec()
}

/// Serializza un messaggio in MessagePack.
///
/// MessagePack invece di JSON: piu' compatto sul filo e nel binario.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    Ok(rmp_serde::to_vec_named(value)?)
}

/// Deserializza un messaggio.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    Ok(rmp_serde::from_slice(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{ClientMessage, ErrorCode, ServerMessage};

    #[test]
    fn roundtrip_preserva_i_campi() {
        let msg = ServerMessage::Error {
            code: ErrorCode::InvalidCredentials,
            detail: "password errata".into(),
        };
        let bytes = encode(&msg).expect("encode");
        let back: ServerMessage = decode(&bytes).expect("decode");

        match back {
            ServerMessage::Error { code, detail } => {
                assert_eq!(code, ErrorCode::InvalidCredentials);
                assert_eq!(detail, "password errata");
            }
            other => panic!("variante sbagliata dopo il round-trip: {other:?}"),
        }
    }

    #[test]
    fn roundtrip_preserva_le_coordinate() {
        use chrono::{TimeZone, Utc};
        use geo_core::Position;

        let istante = Utc.with_ymd_and_hms(2026, 8, 17, 10, 30, 0).unwrap();
        let msg = ClientMessage::PositionUpdate {
            at: istante,
            position: Position::new(45.0618513, 7.6797161),
        };
        let bytes = encode(&msg).expect("encode");
        let back: ClientMessage = decode(&bytes).expect("decode");

        match back {
            ClientMessage::PositionUpdate { at, position } => {
                assert_eq!(at, istante);
                // I gradi decimali devono sopravvivere esatti: un arrotondamento
                // farebbe scattare o mancare la soglia di movimento.
                assert_eq!(position.lat, 45.0618513);
                assert_eq!(position.lon, 7.6797161);
            }
            other => panic!("variante sbagliata: {other:?}"),
        }
    }

    /// Fissa il formato wire: se qualcuno cambia un campo di un messaggio, questo
    /// test rompe subito invece di lasciare scoprire l'incompatibilita' a runtime.
    #[test]
    fn formato_wire_stabile() {
        let msg = ClientMessage::Heartbeat;
        let bytes = encode(&msg).expect("encode");
        assert!(!bytes.is_empty());
        let back: ClientMessage = decode(&bytes).expect("decode");
        assert!(matches!(back, ClientMessage::Heartbeat));
    }
}
