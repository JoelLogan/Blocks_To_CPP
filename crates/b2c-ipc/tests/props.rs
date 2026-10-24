//! Property tests: base64 is a canonical round trip, and the decoder never panics
//! on arbitrary JSON from the webview.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use b2c_ipc::commands::IpcRequest;
use b2c_ipc::dto::{RunInputRequest, RunStartRequest, SettingsPatch};
use b2c_ipc::{decode, decode_base64, encode_base64};
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use serde_json::Value;

/// Arbitrary JSON, biased towards the keys and shapes of real requests so that
/// deep paths through the schema check are reached.
fn json() -> impl Strategy<Value = Value> {
    let key = prop_oneof![
        Just("runId".to_owned()),
        Just("buildId".to_owned()),
        Just("runOptions".to_owned()),
        Just("cols".to_owned()),
        Just("rows".to_owned()),
        Just("data".to_owned()),
        Just("codeStyle".to_owned()),
        Just("indentWidth".to_owned()),
        Just("console".to_owned()),
        Just("scrollbackLines".to_owned()),
        Just("__proto__".to_owned()),
        "[a-zA-Z_]{0,8}",
    ];
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(Value::from),
        "[A-Za-z0-9+/=_ -]{0,12}".prop_map(Value::from),
        Just(Value::from("rn_0123456789abcdef0123456789abcdef")),
        Just(Value::from("bd_0123456789abcdef0123456789abcdef")),
    ];
    leaf.prop_recursive(4, 32, 6, move |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::Array),
            btree_map(key.clone(), inner, 0..5).prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    })
}

proptest! {
    #[test]
    fn base64_round_trips(bytes in vec(any::<u8>(), 0..300)) {
        let text = encode_base64(&bytes);
        prop_assert_eq!(decode_base64(&text).unwrap(), bytes);
    }

    #[test]
    fn base64_accepts_only_canonical_text(text in "[A-Za-z0-9+/=]{0,24}") {
        if let Ok(bytes) = decode_base64(&text) {
            prop_assert_eq!(encode_base64(&bytes), text);
        }
    }

    #[test]
    fn decoding_arbitrary_json_never_panics(value in json()) {
        fn accepted_ones_are_valid<T: IpcRequest>(value: Value) {
            if let Ok(request) = decode::<T>(value) {
                request.validate().unwrap();
                b2c_ipc::schema::check(&serde_json::to_value(&request).unwrap(), T::schema()).unwrap();
            }
        }
        accepted_ones_are_valid::<RunStartRequest>(value.clone());
        accepted_ones_are_valid::<RunInputRequest>(value.clone());
        accepted_ones_are_valid::<SettingsPatch>(value);
    }
}
