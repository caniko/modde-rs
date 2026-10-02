//! Applies `OctoDiff` binary delta patches to a basis file to reconstruct a
//! target file, with bounds and output-size checks to reject malformed patches.
//! See [`apply_patch`] and [`apply_patch_to_writer`].

use std::io::{Cursor, Read, Write};

use anyhow::{Context, Result, bail};

/// Magic bytes identifying an `OctoDiff` binary delta patch.
const OCTODELTA_MAGIC: &[u8; 9] = b"OCTODELTA";

/// Maximum allowed output size for a patch operation (4 GiB).
/// Prevents malformed patches from causing unbounded memory allocation (`DoS`).
const MAX_PATCH_OUTPUT: usize = 4 * 1024 * 1024 * 1024;

/// `OctoDiff` operation types.
const OP_COPY: u8 = 0x60;
const OP_DATA: u8 = 0x80;

/// Apply an `OctoDiff` binary delta patch to a source (basis) file, producing the target file.
///
/// `OctoDiff` delta format:
/// - 9 bytes magic: `OCTODELTA`
/// - 1 byte version: `0x01`
/// - Hash metadata: `type_len(1)` + `name(type_len` bytes) + `hash_len(u32` LE) + `hash(hash_len` bytes)
/// - 3 bytes separator: `>>>`
/// - Repeated operations until end of data:
///   - `0x60` copy: offset(u64 LE) + length(u64 LE) — copy from basis file
///   - `0x80` data: length(u64 LE) + literal(length bytes) — insert literal data
pub fn apply_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    apply_patch_to_writer(source, patch, &mut output)?;
    Ok(output)
}

/// Apply an `OctoDiff` binary delta patch and stream the target bytes to `writer`.
pub fn apply_patch_to_writer<W: Write>(source: &[u8], patch: &[u8], writer: &mut W) -> Result<u64> {
    apply_patch_to_writer_limited(source, patch, writer, None)
}

/// Apply an `OctoDiff` binary delta patch and enforce an expected output size.
///
/// When `expected_output_bytes` is set, the writer fails before writing any
/// operation that would exceed the manifest size and also rejects truncated
/// output after the final operation.
pub fn apply_patch_to_writer_limited<W: Write>(
    source: &[u8],
    patch: &[u8],
    writer: &mut W,
    expected_output_bytes: Option<u64>,
) -> Result<u64> {
    let mut cursor = Cursor::new(patch);
    let max_output_bytes = expected_output_bytes
        .unwrap_or(MAX_PATCH_OUTPUT as u64)
        .min(MAX_PATCH_OUTPUT as u64);

    // Read and verify magic
    let mut magic = [0u8; 9];
    cursor
        .read_exact(&mut magic)
        .context("failed to read patch magic")?;
    if &magic != OCTODELTA_MAGIC {
        bail!("invalid patch magic: expected {OCTODELTA_MAGIC:?}, got {magic:?}");
    }

    // Read version
    let mut version = [0u8; 1];
    cursor.read_exact(&mut version)?;
    if version[0] != 1 {
        bail!("unsupported OctoDiff version: {}", version[0]);
    }

    // Read hash algorithm metadata: type_len(1) + name(type_len) + hash_len(u32 LE) + hash
    let mut type_len = [0u8; 1];
    cursor.read_exact(&mut type_len)?;
    let name_len = type_len[0] as usize;
    let mut name = vec![0u8; name_len];
    cursor.read_exact(&mut name)?;
    let hash_len = read_u32_le(&mut cursor)? as usize;
    let mut _basis_hash = vec![0u8; hash_len];
    cursor.read_exact(&mut _basis_hash)?;

    // Read separator ">>>"
    let mut sep = [0u8; 3];
    cursor
        .read_exact(&mut sep)
        .context("failed to read separator")?;
    if &sep != b">>>" {
        bail!("expected '>>>' separator, got {sep:?}");
    }

    let mut output_len = 0u64;

    loop {
        let mut op_buf = [0u8; 1];
        match cursor.read_exact(&mut op_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e).context("failed to read operation type"),
        }

        match op_buf[0] {
            OP_COPY => {
                let offset = read_u64_le(&mut cursor).context("failed to read copy offset")?;
                let length = read_u64_le(&mut cursor).context("failed to read copy length")?;
                let end = offset
                    .checked_add(length)
                    .context("copy operation offset overflows")?;

                if end > source.len() as u64 {
                    bail!(
                        "copy operation out of bounds: offset={offset}, length={length}, source_len={}",
                        source.len()
                    );
                }
                if output_len.saturating_add(length) > max_output_bytes {
                    bail!("patch output exceeds maximum size of {max_output_bytes} bytes");
                }
                let offset = usize::try_from(offset).context("copy offset does not fit usize")?;
                let end = usize::try_from(end).context("copy end does not fit usize")?;
                writer
                    .write_all(&source[offset..end])
                    .context("failed to write copied patch data")?;
                output_len += length;
            }
            OP_DATA => {
                let length = read_u64_le(&mut cursor).context("failed to read data length")?;
                if output_len.saturating_add(length) > max_output_bytes {
                    bail!("patch output exceeds maximum size of {max_output_bytes} bytes");
                }
                let copied = std::io::copy(&mut cursor.by_ref().take(length), writer)
                    .context("failed to write literal patch data")?;
                if copied != length {
                    bail!("failed to read literal data: expected {length} bytes, got {copied}");
                }
                output_len += length;
            }
            other => {
                bail!("unknown OctoDiff operation type: 0x{other:02x}");
            }
        }
    }

    if let Some(expected) = expected_output_bytes
        && output_len != expected
    {
        bail!("patch output size mismatch: expected {expected} bytes, wrote {output_len}");
    }

    Ok(output_len)
}

/// Read a little-endian u64 from a reader.
fn read_u64_le<R: Read>(reader: &mut R) -> Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Read a little-endian u32 from a reader.
fn read_u32_le<R: Read>(reader: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal `OctoDiff` patch.
    fn build_octodiff_patch(ops: &[(u8, &[u8])]) -> Vec<u8> {
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
        let mut v = Vec::new();
        v.extend_from_slice(&offset.to_le_bytes());
        v.extend_from_slice(&length.to_le_bytes());
        v
    }

    fn data_op(data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(data.len() as u64).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn test_copy_operation() {
        let source = b"Hello, World!";
        let copy = copy_op(0, 5);
        let patch = build_octodiff_patch(&[(OP_COPY, &copy)]);
        let result = apply_patch(source, &patch).unwrap();
        assert_eq!(&result, b"Hello");
    }

    #[test]
    fn test_data_operation() {
        let source = b"";
        let data = data_op(b"abc");
        let patch = build_octodiff_patch(&[(OP_DATA, &data)]);
        let result = apply_patch(source, &patch).unwrap();
        assert_eq!(&result, b"abc");
    }

    #[test]
    fn test_mixed_operations() {
        let source = b"Hello, World!";
        let copy = copy_op(0, 5);
        let data = data_op(b" Rust");
        let patch = build_octodiff_patch(&[(OP_COPY, &copy), (OP_DATA, &data)]);
        let result = apply_patch(source, &patch).unwrap();
        assert_eq!(&result, b"Hello Rust");
    }

    #[test]
    fn test_invalid_magic() {
        let patch = b"BADMAGICXxxxxxxxx";
        let result = apply_patch(b"", patch);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_out_of_bounds() {
        let source = b"Hello";
        let copy = copy_op(0, 10);
        let patch = build_octodiff_patch(&[(OP_COPY, &copy)]);
        let result = apply_patch(source, &patch);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_entire_source() {
        let source = b"Complete source data";
        let copy = copy_op(0, source.len() as u64);
        let patch = build_octodiff_patch(&[(OP_COPY, &copy)]);
        let result = apply_patch(source, &patch).unwrap();
        assert_eq!(&result, source);
    }

    #[test]
    fn test_large_data_insert() {
        let big_data = vec![0xABu8; 10240];
        let data = data_op(&big_data);
        let patch = build_octodiff_patch(&[(OP_DATA, &data)]);
        let result = apply_patch(b"", &patch).unwrap();
        assert_eq!(result.len(), 10240);
        assert!(result.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn streaming_writer_matches_in_memory_output() {
        let source = b"Hello, World!";
        let copy = copy_op(0, 5);
        let data = data_op(b" Rust");
        let patch = build_octodiff_patch(&[(OP_COPY, &copy), (OP_DATA, &data)]);
        let expected = apply_patch(source, &patch).unwrap();
        let mut streamed = Vec::new();
        let written = apply_patch_to_writer(source, &patch, &mut streamed).unwrap();
        assert_eq!(written, expected.len() as u64);
        assert_eq!(streamed, expected);
    }

    #[test]
    fn streaming_writer_reports_truncated_literal() {
        let mut literal = Vec::new();
        literal.extend_from_slice(&8u64.to_le_bytes());
        literal.extend_from_slice(b"abc");
        let patch = build_octodiff_patch(&[(OP_DATA, &literal)]);
        let mut streamed = Vec::new();
        let result = apply_patch_to_writer(b"", &patch, &mut streamed);
        assert!(result.is_err());
    }

    #[test]
    fn streaming_writer_rejects_output_larger_than_expected_before_writing_operation() {
        let patch = build_octodiff_patch(&[(OP_DATA, &data_op(b"abcdef"))]);
        let mut streamed = Vec::new();
        let result = apply_patch_to_writer_limited(b"", &patch, &mut streamed, Some(3));
        assert!(result.is_err());
        assert_eq!(streamed, [] as [u8; 0]);
    }

    #[test]
    fn streaming_writer_rejects_output_smaller_than_expected() {
        let patch = build_octodiff_patch(&[(OP_DATA, &data_op(b"abc"))]);
        let mut streamed = Vec::new();
        let result = apply_patch_to_writer_limited(b"", &patch, &mut streamed, Some(6));
        assert!(result.is_err());
        assert_eq!(streamed, b"abc");
    }

    #[test]
    fn streaming_writer_accepts_exact_expected_output_size() {
        let patch = build_octodiff_patch(&[(OP_DATA, &data_op(b"abc"))]);
        let mut streamed = Vec::new();
        let written = apply_patch_to_writer_limited(b"", &patch, &mut streamed, Some(3)).unwrap();
        assert_eq!(written, 3);
        assert_eq!(streamed, b"abc");
    }

    #[test]
    fn test_multiple_copy_operations() {
        let source = b"ABCDEFGHIJ";
        let copy1 = copy_op(0, 3);
        let copy2 = copy_op(6, 3);
        let patch = build_octodiff_patch(&[(OP_COPY, &copy1), (OP_COPY, &copy2)]);
        let result = apply_patch(source, &patch).unwrap();
        assert_eq!(&result, b"ABCGHI");
    }

    #[test]
    fn test_empty_patch() {
        let patch = build_octodiff_patch(&[]);
        let result = apply_patch(b"some source", &patch).unwrap();
        assert_eq!(result, [] as [u8; 0]);
    }
}
