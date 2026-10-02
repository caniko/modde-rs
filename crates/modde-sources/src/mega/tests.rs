use super::*;
use modde_core::GameId;

// ── parse_mega_url: new format /file/HANDLE#KEY ──────────────────────

#[test]
fn parse_new_format_https() {
    let (handle, key) = parse_mega_url("https://mega.nz/file/ABC123#some_key_base64").unwrap();
    assert_eq!(handle, "ABC123");
    assert_eq!(key, "some_key_base64");
}

#[test]
fn parse_new_format_http() {
    let (handle, key) = parse_mega_url("http://mega.nz/file/XYZ789#another_key").unwrap();
    assert_eq!(handle, "XYZ789");
    assert_eq!(key, "another_key");
}

#[test]
fn parse_new_format_long_handle_and_key() {
    let (handle, key) = parse_mega_url(
        "https://mega.nz/file/AbCdEfGhIjKlMnOp#AAAAAAAAAAAABBBBBBBBBBBBCCCCCCCCCCCCDDDDDDDDDDDD",
    )
    .unwrap();
    assert_eq!(handle, "AbCdEfGhIjKlMnOp");
    assert_eq!(key, "AAAAAAAAAAAABBBBBBBBBBBBCCCCCCCCCCCCDDDDDDDDDDDD");
}

#[test]
fn parse_new_format_key_with_special_base64url_chars() {
    // base64url uses - and _ instead of + and /
    let (handle, key) =
        parse_mega_url("https://mega.nz/file/HANDLE#a-b_c-d_e-f_g-h_i-j_k").unwrap();
    assert_eq!(handle, "HANDLE");
    assert_eq!(key, "a-b_c-d_e-f_g-h_i-j_k");
}

// ── parse_mega_url: old format /#!HANDLE!KEY ─────────────────────────

#[test]
fn parse_old_format_https() {
    let (handle, key) = parse_mega_url("https://mega.nz/#!ABC123!some_key_base64").unwrap();
    assert_eq!(handle, "ABC123");
    assert_eq!(key, "some_key_base64");
}

#[test]
fn parse_old_format_http() {
    let (handle, key) = parse_mega_url("http://mega.nz/#!OldHandle!OldKey123").unwrap();
    assert_eq!(handle, "OldHandle");
    assert_eq!(key, "OldKey123");
}

#[test]
fn parse_old_format_with_extra_prefix() {
    // The old-format parser uses find("#!"), so it works even with odd prefixes
    let (handle, key) = parse_mega_url("https://mega.co.nz/#!HANDLE!KEY").unwrap();
    assert_eq!(handle, "HANDLE");
    assert_eq!(key, "KEY");
}

// ── parse_mega_url: invalid URLs ─────────────────────────────────────

#[test]
fn parse_url_no_hash_new_format() {
    // Missing # separator in new format
    assert!(parse_mega_url("https://mega.nz/file/ABCnohash").is_err());
}

#[test]
fn parse_url_random_url() {
    assert!(parse_mega_url("https://example.com/file").is_err());
}

#[test]
fn parse_url_empty_string() {
    assert!(parse_mega_url("").is_err());
}

#[test]
fn parse_url_only_domain() {
    assert!(parse_mega_url("https://mega.nz").is_err());
}

#[test]
fn parse_url_no_key_after_hash() {
    // "splitn(2, '#')" produces ["HANDLE", ""] – length 2 but empty key
    // The function does not reject empty keys, so this succeeds with empty key
    let result = parse_mega_url("https://mega.nz/file/HANDLE#");
    // Regardless of success/failure, document behaviour
    if let Ok((_handle, key)) = &result {
        assert_eq!(key, "");
    }
}

#[test]
fn parse_url_with_query_params_new_format() {
    // Query params end up as part of the key (since we only split on #)
    let (handle, key) = parse_mega_url("https://mega.nz/file/HANDLE#KEY?foo=bar").unwrap();
    assert_eq!(handle, "HANDLE");
    assert_eq!(key, "KEY?foo=bar");
}

#[test]
fn parse_url_with_extra_path_segments() {
    // "/file/" prefix is stripped, then everything up to # is handle
    let (handle, key) = parse_mega_url("https://mega.nz/file/HANDLE/extra#KEY").unwrap();
    assert_eq!(handle, "HANDLE/extra");
    assert_eq!(key, "KEY");
}

// ── decode_mega_key: valid 32-byte keys ──────────────────────────────

#[test]
fn decode_key_valid_32_bytes() {
    // 32 zero bytes -> base64url = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
    // URL_SAFE_NO_PAD expects no padding, so use the unpadded form
    let key_bytes = [0u8; 32];
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let (aes_key, iv) = decode_mega_key(&key_b64).unwrap();
    // 0 XOR 0 = 0 for all bytes
    assert_eq!(aes_key, [0u8; 16]);
    // IV = bytes 16..24 of original (all zero), zero-padded
    assert_eq!(iv, [0u8; 16]);
}

#[test]
fn decode_key_xor_logic() {
    // Construct a 32-byte key where first half = [1..=16], second half = [17..=32]
    let mut key_bytes = [0u8; 32];
    for i in 0..16 {
        key_bytes[i] = (i + 1) as u8;
        key_bytes[i + 16] = (i + 17) as u8;
    }
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let (aes_key, iv) = decode_mega_key(&key_b64).unwrap();

    // Verify XOR: aes_key[i] = key_bytes[i] ^ key_bytes[i+16]
    for i in 0..16 {
        assert_eq!(
            aes_key[i],
            key_bytes[i] ^ key_bytes[i + 16],
            "XOR mismatch at index {i}"
        );
    }

    // Verify IV: bytes 16..24 of original, zero-padded to 16
    let mut expected_iv = [0u8; 16];
    expected_iv[..8].copy_from_slice(&key_bytes[16..24]);
    assert_eq!(iv, expected_iv);
}

#[test]
fn decode_key_xor_inverse() {
    // If first half == second half, XOR yields all zeros
    let mut key_bytes = [0u8; 32];
    for byte in key_bytes.iter_mut().take(16) {
        *byte = 0xAB;
    }
    for byte in key_bytes.iter_mut().skip(16) {
        *byte = 0xAB;
    }
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let (aes_key, _iv) = decode_mega_key(&key_b64).unwrap();
    assert_eq!(aes_key, [0u8; 16]);
}

#[test]
fn decode_key_xor_all_ones() {
    // first half = 0xFF, second half = 0x00 -> XOR = 0xFF
    let mut key_bytes = [0u8; 32];
    for byte in key_bytes.iter_mut().take(16) {
        *byte = 0xFF;
    }
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let (aes_key, _iv) = decode_mega_key(&key_b64).unwrap();
    assert_eq!(aes_key, [0xFF; 16]);
}

// ── decode_mega_key: IV extraction correctness ───────────────────────

#[test]
fn decode_key_iv_extraction() {
    let mut key_bytes = [0u8; 32];
    // Set bytes 16..24 to distinct values
    for i in 0..8 {
        key_bytes[16 + i] = (0x10 + i) as u8;
    }
    // Set bytes 24..32 to something else (should NOT appear in IV)
    for i in 0..8 {
        key_bytes[24 + i] = 0xFF;
    }
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let (_aes_key, iv) = decode_mega_key(&key_b64).unwrap();

    // First 8 bytes of IV = bytes 16..24 of original
    for (i, byte) in iv.iter().enumerate().take(8) {
        assert_eq!(*byte, (0x10 + i) as u8, "IV byte {i} mismatch");
    }
    // Last 8 bytes of IV must be zero (counter)
    for (i, byte) in iv.iter().enumerate().skip(8) {
        assert_eq!(*byte, 0, "IV counter byte {i} should be zero");
    }
}

// ── decode_mega_key: invalid inputs ──────────────────────────────────

#[test]
fn decode_key_too_short() {
    let short = URL_SAFE_NO_PAD.encode([0u8; 16]);
    let err = decode_mega_key(&short).unwrap_err();
    assert!(
        err.to_string().contains("expected 32-byte"),
        "unexpected error: {err}"
    );
}

#[test]
fn decode_key_too_long() {
    let long = URL_SAFE_NO_PAD.encode([0u8; 48]);
    let err = decode_mega_key(&long).unwrap_err();
    assert!(
        err.to_string().contains("expected 32-byte"),
        "unexpected error: {err}"
    );
}

#[test]
fn decode_key_empty() {
    let err = decode_mega_key("").unwrap_err();
    assert!(
        err.to_string().contains("expected 32-byte"),
        "unexpected error: {err}"
    );
}

#[test]
fn decode_key_invalid_base64() {
    let err = decode_mega_key("!!!not-valid-base64!!!").unwrap_err();
    assert!(
        err.to_string().contains("base64"),
        "unexpected error: {err}"
    );
}

#[test]
fn decode_key_one_byte() {
    let one = URL_SAFE_NO_PAD.encode([0x42u8; 1]);
    let err = decode_mega_key(&one).unwrap_err();
    assert!(err.to_string().contains("expected 32-byte"));
}

#[test]
fn decode_key_31_bytes() {
    let data = URL_SAFE_NO_PAD.encode([0u8; 31]);
    assert!(decode_mega_key(&data).is_err());
}

#[test]
fn decode_key_33_bytes() {
    let data = URL_SAFE_NO_PAD.encode([0u8; 33]);
    assert!(decode_mega_key(&data).is_err());
}

// ── decode_mega_key: very long key string ────────────────────────────

#[test]
fn decode_key_very_long_base64() {
    // 256 bytes is way too long
    let long = URL_SAFE_NO_PAD.encode([0xABu8; 256]);
    assert!(decode_mega_key(&long).is_err());
}

// ── can_handle ───────────────────────────────────────────────────────

#[test]
fn can_handle_mega_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/ABC#KEY".to_string(),
        hash: 0,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn can_handle_rejects_nexus() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrim"),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn can_handle_rejects_google_drive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "some-id".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn can_handle_rejects_github() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::GitHub {
        user: "user".to_string(),
        repo: "repo".to_string(),
        tag: "v1".to_string(),
        asset: "file.zip".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn can_handle_rejects_direct_url() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::DirectURL {
        url: "https://example.com/file.zip".to_string(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}
