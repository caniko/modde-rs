use modde_sources::wabbajack::patcher::apply_patch;

const OCTODELTA_MAGIC: &[u8; 9] = b"OCTODELTA";
const OP_COPY: u8 = 0x60;
const OP_DATA: u8 = 0x80;

fn build_patch(ops: &[(u8, &[u8])]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(OCTODELTA_MAGIC);
    patch.push(1); // version
    // Hash metadata: SHA1
    patch.push(4); // name length
    patch.extend_from_slice(b"SHA1");
    patch.extend_from_slice(&20u32.to_le_bytes()); // hash length
    patch.extend_from_slice(&[0u8; 20]); // dummy hash
    patch.extend_from_slice(b">>>"); // separator
    for (op_type, payload) in ops {
        patch.push(*op_type);
        patch.extend_from_slice(payload);
    }
    patch
}

fn copy_op(offset: u64, length: u64) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&offset.to_le_bytes());
    data.extend_from_slice(&length.to_le_bytes());
    data
}

fn data_op(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(data);
    v
}

// ── Zero-length COPY operation ──────────────────────────────────────

#[test]
fn test_patch_zero_length_copy_followed_by_insert() {
    let source = b"ABCDEF";
    let zero_copy = copy_op(0, 0);
    let ins = data_op(b"XYZ");
    let patch = build_patch(&[(OP_COPY, &zero_copy), (OP_DATA, &ins)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"XYZ");
}

#[test]
fn test_patch_zero_length_copy_at_various_offsets() {
    let source = b"ABCDEF";
    let zero_copy = copy_op(3, 0);
    let patch = build_patch(&[(OP_COPY, &zero_copy)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

// ── Zero-length INSERT operation ────────────────────────────────────

#[test]
fn test_patch_zero_length_insert_between_copies() {
    let source = b"ABCDEF";
    let cop1 = copy_op(0, 3);
    let cop2 = copy_op(3, 3);
    let ins = data_op(b"");
    let patch = build_patch(&[(OP_COPY, &cop1), (OP_DATA, &ins), (OP_COPY, &cop2)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"ABCDEF");
}

// ── Patch producing empty output ────────────────────────────────────

#[test]
fn test_patch_output_size_zero_no_ops() {
    let patch = build_patch(&[]);
    let result = apply_patch(b"some source data", &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

#[test]
fn test_patch_output_size_zero_with_zero_length_ops() {
    let zero_copy = copy_op(0, 0);
    let ins = data_op(b"");
    let patch = build_patch(&[(OP_COPY, &zero_copy), (OP_DATA, &ins)]);
    let result = apply_patch(b"source", &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

// ── Multiple consecutive COPY operations ────────────────────────────

#[test]
fn test_patch_five_consecutive_copies() {
    let source = b"ABCDEFGHIJKLMNO";
    let cop1 = copy_op(0, 3);
    let cop2 = copy_op(3, 3);
    let cop3 = copy_op(6, 3);
    let cop4 = copy_op(9, 3);
    let cop5 = copy_op(12, 3);
    let patch = build_patch(&[
        (OP_COPY, &cop1),
        (OP_COPY, &cop2),
        (OP_COPY, &cop3),
        (OP_COPY, &cop4),
        (OP_COPY, &cop5),
    ]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"ABCDEFGHIJKLMNO");
}

// ── Multiple consecutive INSERT operations ──────────────────────────

#[test]
fn test_patch_multiple_consecutive_inserts() {
    let d1 = data_op(b"Hello");
    let d2 = data_op(b", ");
    let d3 = data_op(b"World");
    let d4 = data_op(b"!!!");
    let patch = build_patch(&[
        (OP_DATA, &d1),
        (OP_DATA, &d2),
        (OP_DATA, &d3),
        (OP_DATA, &d4),
    ]);
    let result = apply_patch(b"", &patch).unwrap();
    assert_eq!(&result, b"Hello, World!!!");
}

// ── Copy from very end of source (last byte) ────────────────────────

#[test]
fn test_patch_copy_last_single_byte() {
    let source = b"ABCDE";
    let cop = copy_op(4, 1);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"E");
}

#[test]
fn test_patch_copy_last_two_bytes() {
    let source = b"Hello!";
    let cop = copy_op(4, 2);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"o!");
}

// ── Very large INSERT data (1MB+) ──────────────────────────────────

#[test]
fn test_patch_large_insert_1mb() {
    let size = 1_048_576;
    let big = vec![0xCDu8; size];
    let ins = data_op(&big);
    let patch = build_patch(&[(OP_DATA, &ins)]);
    let result = apply_patch(b"", &patch).unwrap();
    assert_eq!(result.len(), size);
    assert!(result.iter().all(|&b| b == 0xCD));
}

// ── Patch that copies entire source unchanged ──────────────────────

#[test]
fn test_patch_identity_copy_small() {
    let source = b"Exact copy of source";
    let cop = copy_op(0, source.len() as u64);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, source);
}

#[test]
fn test_patch_identity_copy_large() {
    let source: Vec<u8> = (0..50_000).map(|i| (i % 256) as u8).collect();
    let cop = copy_op(0, source.len() as u64);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(&source, &patch).unwrap();
    assert_eq!(result, source);
}

// ── Source modification detection ───────────────────────────────────

#[test]
fn test_patch_wrong_source_produces_wrong_output() {
    let correct_source = b"Hello, World!";
    let wrong_source = b"Goodbye World!";

    let cop = copy_op(0, 5);
    let patch = build_patch(&[(OP_COPY, &cop)]);

    let correct_result = apply_patch(correct_source, &patch).unwrap();
    assert_eq!(&correct_result, b"Hello");

    let wrong_result = apply_patch(wrong_source, &patch).unwrap();
    assert_eq!(&wrong_result, b"Goodb");
    assert_ne!(correct_result, wrong_result);
}

#[test]
fn test_patch_truncated_source_causes_error() {
    let full_source = b"ABCDEFGHIJ";
    let short_source = b"ABC";

    let cop = copy_op(0, 10);
    let patch = build_patch(&[(OP_COPY, &cop)]);

    let result = apply_patch(full_source, &patch).unwrap();
    assert_eq!(&result, b"ABCDEFGHIJ");

    let err = apply_patch(short_source, &patch);
    assert!(err.is_err());
    assert!(err.unwrap_err().to_string().contains("out of bounds"));
}

// ── Reverse order copy ──────────────────────────────────────────────

#[test]
fn test_patch_reverse_order_copies() {
    let source = b"ABCDEF";
    let cop1 = copy_op(3, 3);
    let cop2 = copy_op(0, 3);
    let patch = build_patch(&[(OP_COPY, &cop1), (OP_COPY, &cop2)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"DEFABC");
}

// ── Mix of small copies and inserts ─────────────────────────────────

#[test]
fn test_patch_interleaved_single_byte_ops() {
    let source = b"ABCD";
    let cop_a = copy_op(0, 1);
    let cop_c = copy_op(2, 1);
    let ins_x = data_op(b"X");
    let ins_y = data_op(b"Y");
    let patch = build_patch(&[
        (OP_COPY, &cop_a),
        (OP_DATA, &ins_x),
        (OP_COPY, &cop_c),
        (OP_DATA, &ins_y),
    ]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"AXCY");
}
