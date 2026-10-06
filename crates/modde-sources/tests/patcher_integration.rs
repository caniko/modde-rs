use modde_sources::wabbajack::patcher::apply_patch;

// ── Integration tests for OctoDiff binary delta patching ────────────

const OCTODELTA_MAGIC: &[u8; 9] = b"OCTODELTA";
const OP_COPY: u8 = 0x60;
const OP_DATA: u8 = 0x80;

fn build_patch(ops: &[(u8, &[u8])]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(OCTODELTA_MAGIC);
    patch.push(1); // version
    patch.push(4); // hash name length
    patch.extend_from_slice(b"SHA1");
    patch.extend_from_slice(&20u32.to_le_bytes());
    patch.extend_from_slice(&[0u8; 20]); // dummy hash
    patch.extend_from_slice(b">>>");
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

// ── Complex patching scenarios ──────────────────────────────────────

#[test]
fn test_patch_reconstruct_file_from_fragments() {
    let source = b"The quick brown fox jumps over the lazy dog";
    let target = b"The lazy dog jumps over the quick brown fox";

    let cop1 = copy_op(0, 4); // "The "
    let cop2 = copy_op(35, 8); // "lazy dog"
    let cop3 = copy_op(19, 16); // " jumps over the "
    let cop4 = copy_op(4, 15); // "quick brown fox"

    let patch = build_patch(&[
        (OP_COPY, &cop1),
        (OP_COPY, &cop2),
        (OP_COPY, &cop3),
        (OP_COPY, &cop4),
    ]);

    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(result, target);
}

#[test]
fn test_patch_insert_between_copies() {
    let source = b"ABCDEFGHIJ";
    let cop1 = copy_op(0, 3);
    let ins = data_op(b"XYZ");
    let cop2 = copy_op(7, 3);
    let patch = build_patch(&[(OP_COPY, &cop1), (OP_DATA, &ins), (OP_COPY, &cop2)]);

    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"ABCXYZHIJ");
}

#[test]
fn test_patch_overlapping_copies() {
    let source = b"ABCDEFGH";
    let cop1 = copy_op(0, 5);
    let cop2 = copy_op(2, 6);
    let patch = build_patch(&[(OP_COPY, &cop1), (OP_COPY, &cop2)]);

    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"ABCDECDEFGH");
}

#[test]
fn test_patch_large_file_reconstruction() {
    let source: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    let insert_data: Vec<u8> = vec![0xFF; 50_000];

    let cop = copy_op(0, 50_000);
    let ins = data_op(&insert_data);
    let patch = build_patch(&[(OP_COPY, &cop), (OP_DATA, &ins)]);

    let result = apply_patch(&source, &patch).unwrap();
    assert_eq!(result.len(), 100_000);
    assert_eq!(&result[..50_000], &source[..50_000]);
    assert!(result[50_000..].iter().all(|&b| b == 0xFF));
}

// ── Error handling edge cases ───────────────────────────────────────

#[test]
fn test_patch_empty_data() {
    let result = apply_patch(b"", b"");
    assert!(result.is_err(), "empty patch should fail (no magic)");
}

#[test]
fn test_patch_only_magic() {
    let result = apply_patch(b"", b"OCTODELTA");
    assert!(result.is_err(), "patch with only magic should fail");
}

#[test]
fn test_patch_empty_ops() {
    let patch = build_patch(&[]);
    let result = apply_patch(b"source data", &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

#[test]
fn test_patch_copy_at_exact_boundary() {
    let source = b"12345";
    let cop = copy_op(0, 5);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, source);
}

#[test]
fn test_patch_copy_one_byte_past_boundary() {
    let source = b"12345";
    let cop = copy_op(0, 6);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("out of bounds"));
}

#[test]
fn test_patch_copy_offset_at_end() {
    let source = b"12345";
    let cop = copy_op(5, 0);
    let patch = build_patch(&[(OP_COPY, &cop)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

#[test]
fn test_patch_all_operation_types_in_sequence() {
    let source = b"SOURCE";
    let cop1 = copy_op(0, 3);
    let ins = data_op(b"NEW");
    let cop2 = copy_op(3, 3);
    let patch = build_patch(&[(OP_COPY, &cop1), (OP_DATA, &ins), (OP_COPY, &cop2)]);

    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"SOUNEWRCE");
}
