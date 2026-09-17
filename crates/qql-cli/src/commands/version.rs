//! `qql version`.

use crate::output;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn build_version_response() -> output::VersionResponse {
    let features = [
        ("grpc", cfg!(feature = "grpc")),
        ("rest", cfg!(feature = "rest")),
        ("edge", cfg!(feature = "edge")),
        ("fastembed", cfg!(feature = "fastembed")),
    ]
    .into_iter()
    .filter(|(_, on)| *on)
    .map(|(name, _)| name.to_string())
    .collect::<Vec<_>>();
    let is_full = cfg!(feature = "edge") && cfg!(feature = "fastembed");
    let is_partial = cfg!(feature = "edge") || cfg!(feature = "fastembed");
    let edition = if is_full {
        "full"
    } else if is_partial {
        "custom"
    } else {
        "standard"
    };
    output::VersionResponse {
        ok: true,
        command: "version".to_string(),
        version: VERSION.to_string(),
        edition: edition.to_string(),
        features,
        message: format!("qql version {} ({})", VERSION, edition),
    }
}

pub fn handle_version() -> Result<(), Box<dyn std::error::Error>> {
    let resp = build_version_response();
    let s = serde_json::to_string_pretty(&resp)?;
    println!("{}", s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_response_edition_and_serialization() {
        let resp = build_version_response();
        assert!(resp.ok);
        assert_eq!(resp.command, "version");
        assert_eq!(resp.version, VERSION);

        let is_full = cfg!(feature = "edge") && cfg!(feature = "fastembed");
        let is_partial = cfg!(feature = "edge") || cfg!(feature = "fastembed");
        let expected_edition = if is_full {
            "full"
        } else if is_partial {
            "custom"
        } else {
            "standard"
        };
        assert_eq!(resp.edition, expected_edition);
        assert_eq!(
            resp.message,
            format!("qql version {} ({})", VERSION, expected_edition)
        );

        // Verify JSON roundtrip
        let json_str = serde_json::to_string_pretty(&resp).expect("must serialize");
        let deserialized: output::VersionResponse =
            serde_json::from_str(&json_str).expect("must deserialize VersionResponse JSON");
        assert_eq!(deserialized, resp);
    }
}
