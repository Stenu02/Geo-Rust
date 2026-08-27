use geo_admin::ui::{MockData, FleetEntry, ChatEntry};
use geo_core::analytics::MovementReport;
use geo_core::position::{UserId, Timestamped, Position};
use geo_core::timerange::{RangeSpec, TimeRange};
use chrono::{Utc, Duration};
use std::fs;

fn main() {
    let now = Utc::now();
    let rep = MovementReport {
        user: UserId(100),
        spec: RangeSpec::Today,
        range: TimeRange::new(now - Duration::hours(10), now),
        track: vec![
            Timestamped::new(now - Duration::minutes(30), Position::new(45.0, 7.0)),
            Timestamped::new(now, Position::new(45.1, 7.1)),
        ],
        distance_km: 15.4,
        avg_speed_moving_kmh: 70.2,
        avg_speed_overall_kmh: 40.5,
        moving_time: Duration::minutes(45),
        paused_time: Duration::minutes(15),
        disconnected_time: Duration::minutes(5),
        pause_count: 2,
    };

    let data = MockData {
        fleet: vec![
            FleetEntry {
                user_id: "P3_Camion1".to_string(),
                state: "In Movimento".to_string(),
                last_pos: "45.06, 7.68".to_string(),
                last_update: "10s fa".to_string(),
            },
            FleetEntry {
                user_id: "P3_Camion2".to_string(),
                state: "Fermo".to_string(),
                last_pos: "45.10, 7.50".to_string(),
                last_update: "5m fa".to_string(),
            },
        ],
        chats: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "P3_Camion1".to_string(),
                vec![
                    ChatEntry { sender: "P3_Camion1".to_string(), text: "Sto per arrivare al casello.".to_string() },
                    ChatEntry { sender: "ADMIN".to_string(), text: "Ricevuto, procedi pure.".to_string() },
                ]
            );
            m.insert(
                "P3_Camion2".to_string(),
                vec![
                    ChatEntry { sender: "P3_Camion2".to_string(), text: "Gomma bucata, mi fermo.".to_string() },
                    ChatEntry { sender: "ADMIN".to_string(), text: "Mando subito l'assistenza.".to_string() },
                ]
            );
            m
        },
        query_report: rep,
    };

    let bytes = geo_protocol::codec::encode(&data).expect("Errore durante la codifica binaria");
    fs::write("assets/mock_admin_data.bin", bytes).unwrap();
    println!("File mock_admin_data.bin (MessagePack) rigenerato con successo!");
}
