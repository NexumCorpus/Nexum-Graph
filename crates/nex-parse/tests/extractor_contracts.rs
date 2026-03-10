use nex_core::{SemanticId, SemanticUnit};
use nex_parse::SemanticExtractor;
use proptest::prelude::*;
use std::collections::HashSet;
use std::path::Path;

fn expected_id(unit: &SemanticUnit) -> SemanticId {
    let digest = blake3::hash(
        format!(
            "{}:{}:{}",
            unit.qualified_name,
            unit.file_path.display(),
            unit.body_hash
        )
        .as_bytes(),
    );
    *digest.as_bytes()
}

fn assert_extraction_contract(extractor: Box<dyn SemanticExtractor>, path: &Path, source: &[u8]) {
    let units = extractor
        .extract(path, source)
        .expect("extractor should handle arbitrary bytes");
    let deps = extractor
        .dependencies(&units, source)
        .expect("dependency extraction should handle arbitrary bytes");

    let unit_ids: HashSet<_> = units.iter().map(|unit| unit.id).collect();
    for unit in &units {
        assert_eq!(unit.file_path, path);
        assert!(unit.byte_range.start <= unit.byte_range.end);
        assert!(unit.byte_range.end <= source.len());
        assert_eq!(unit.id, expected_id(unit));
    }

    for (from, to, _) in deps {
        assert!(unit_ids.contains(&from));
        assert!(unit_ids.contains(&to));
    }
}

#[test]
fn typescript_semantic_id_tracks_body_hash_not_signature_hash() {
    let extractor = nex_parse::typescript_extractor();

    let baseline = extractor
        .extract(
            Path::new("handler.ts"),
            br#"function validate(input: string): boolean { return input.length > 0; }"#,
        )
        .unwrap();
    let signature_only = extractor
        .extract(
            Path::new("handler.ts"),
            br#"function validate(input: string, strict: boolean): boolean { return input.length > 0; }"#,
        )
        .unwrap();
    let body_changed = extractor
        .extract(
            Path::new("handler.ts"),
            br#"function validate(input: string): boolean { console.log(input); return input.length > 0; }"#,
        )
        .unwrap();

    assert_eq!(baseline[0].id, signature_only[0].id);
    assert_ne!(baseline[0].id, body_changed[0].id);
}

#[test]
fn python_semantic_id_tracks_body_hash_not_signature_hash() {
    let extractor = nex_parse::python_extractor();

    let baseline = extractor
        .extract(
            Path::new("handler.py"),
            br#"def validate(input: str) -> bool:
    return len(input) > 0
"#,
        )
        .unwrap();
    let signature_only = extractor
        .extract(
            Path::new("handler.py"),
            br#"def validate(input: str, strict: bool) -> bool:
    return len(input) > 0
"#,
        )
        .unwrap();
    let body_changed = extractor
        .extract(
            Path::new("handler.py"),
            br#"def validate(input: str) -> bool:
    print(input)
    return len(input) > 0
"#,
        )
        .unwrap();

    assert_eq!(baseline[0].id, signature_only[0].id);
    assert_ne!(baseline[0].id, body_changed[0].id);
}

#[test]
fn rust_semantic_id_tracks_body_hash_not_signature_hash() {
    let extractor = nex_parse::rust_extractor();

    let baseline = extractor
        .extract(
            Path::new("handler.rs"),
            br#"fn validate(input: &str) -> bool { !input.is_empty() }"#,
        )
        .unwrap();
    let signature_only = extractor
        .extract(
            Path::new("handler.rs"),
            br#"fn validate(input: &str, strict: bool) -> bool { !input.is_empty() }"#,
        )
        .unwrap();
    let body_changed = extractor
        .extract(
            Path::new("handler.rs"),
            br#"fn validate(input: &str) -> bool { println!("{}", input); !input.is_empty() }"#,
        )
        .unwrap();

    assert_eq!(baseline[0].id, signature_only[0].id);
    assert_ne!(baseline[0].id, body_changed[0].id);
}

proptest! {
    #[test]
    fn typescript_extractor_handles_arbitrary_bytes(source in proptest::collection::vec(any::<u8>(), 0..256)) {
        assert_extraction_contract(nex_parse::typescript_extractor(), Path::new("fuzz.ts"), &source);
    }

    #[test]
    fn python_extractor_handles_arbitrary_bytes(source in proptest::collection::vec(any::<u8>(), 0..256)) {
        assert_extraction_contract(nex_parse::python_extractor(), Path::new("fuzz.py"), &source);
    }

    #[test]
    fn rust_extractor_handles_arbitrary_bytes(source in proptest::collection::vec(any::<u8>(), 0..256)) {
        assert_extraction_contract(nex_parse::rust_extractor(), Path::new("fuzz.rs"), &source);
    }
}
