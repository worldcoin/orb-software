use std::io::Read;

use orb_pcp::archive::{self, Error};

#[test]
fn tar_preserves_entry_order_and_payloads() {
    let payload = vec![0xab; 513];
    let inputs = [("z.bin", b"".as_slice()), ("a.bin", payload.as_slice())];
    let bytes = archive::encode_tar(123, inputs).unwrap();
    let mut reader = tar::Archive::new(bytes.as_slice());
    let mut entries = reader.entries().unwrap();
    for (name, expected) in inputs {
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(entry.path_bytes(), name.as_bytes());
        let mut actual = Vec::new();
        entry.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, expected);
    }
    assert!(entries.next().is_none());
    assert_eq!(bytes.len(), 3072);
    assert!(bytes[1537..].iter().all(|byte| *byte == 0));
    assert_ne!(
        bytes,
        archive::encode_tar(123, inputs.into_iter().rev()).unwrap()
    );
}

#[test]
fn tar_headers_match_the_pcp_profile() {
    let bytes = archive::encode_tar(123, [("example.bin", b"abc".as_slice())]).unwrap();
    let mut reader = tar::Archive::new(bytes.as_slice());
    let entry = reader.entries().unwrap().next().unwrap().unwrap();
    let header = entry.header();
    assert!(header.as_gnu().is_some());
    assert_eq!(header.uid().unwrap(), 0);
    assert_eq!(header.gid().unwrap(), 0);
    assert_eq!(header.mode().unwrap(), 0o644);
    assert_eq!(header.size().unwrap(), 3);
    assert_eq!(header.mtime().unwrap(), 123);
    assert_eq!(header.device_major().unwrap(), Some(0));
    assert_eq!(header.device_minor().unwrap(), Some(0));
    assert_eq!(header.username().unwrap(), Some(""));
    assert_eq!(header.groupname().unwrap(), Some(""));
    assert!(header.link_name().unwrap().is_none());
    assert_eq!(bytes[156], 0);
    assert_eq!(&bytes[257..265], b"ustar  \0");
    assert_eq!(&bytes[329..337], b"0000000\0");
    assert_eq!(&bytes[337..345], b"0000000\0");
    assert_eq!(&bytes[512..515], b"abc");
    assert_eq!(bytes.len(), 2048);
    assert!(bytes[515..].iter().all(|byte| *byte == 0));
}

#[test]
fn empty_tar_is_two_zero_blocks() {
    assert_eq!(archive::encode_tar(0, []).unwrap(), vec![0; 1024]);
}

#[test]
fn tar_accepts_full_width_and_utf8_names_without_extensions() {
    for name in ["a".repeat(100), "λ".repeat(50)] {
        let bytes = archive::encode_tar(0, [(name.as_str(), b"".as_slice())]).unwrap();
        assert_eq!(&bytes[..100], name.as_bytes());
        assert_eq!(bytes.len(), 1536);
    }
}

#[test]
fn filenames_are_validated_before_encoding() {
    for name in [
        "",
        ".",
        "..",
        "../file",
        "/file",
        "a/b",
        "a\\b",
        "C:file",
        "bad\0name",
        "bad\nname",
        "bad\tname",
        "bad\u{0085}name",
        &"a".repeat(101),
        &"λ".repeat(51),
    ] {
        assert!(matches!(
            archive::encode_tar(0, [(name, b"".as_slice())]),
            Err(Error::InvalidName)
        ));
        assert!(matches!(
            archive::compress(b"", 0, name),
            Err(Error::InvalidName)
        ));
    }
}

#[test]
fn duplicate_filenames_are_rejected() {
    for second in [b"a".as_slice(), b"b".as_slice()] {
        assert!(matches!(
            archive::encode_tar(0, [("file", b"a".as_slice()), ("file", second)]),
            Err(Error::DuplicateName)
        ));
    }
}

#[test]
fn gzip_preserves_metadata_and_round_trips_tar_bytes() {
    let tar = archive::encode_tar(123, [("example.bin", b"abc".as_slice())]).unwrap();
    for filename in ["tier0.tar.gz", "tier1.tar.gz", "tier2.tar.gz"] {
        let bytes = archive::compress(&tar, 123, filename).unwrap();
        assert_eq!(&bytes[..10], &[31, 139, 8, 8, 123, 0, 0, 0, 2, 255]);
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        let header = decoder.header().unwrap();
        assert_eq!(header.filename(), Some(filename.as_bytes()));
        assert_eq!(header.mtime(), 123);
        assert!(header.comment().is_none());
        assert!(header.extra().is_none());
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, tar);
    }
}

#[test]
fn gzip_handles_empty_input_and_timestamp_boundaries() {
    for timestamp in [0, u64::from(u32::MAX)] {
        let bytes = archive::compress(b"", timestamp, "tier0.tar.gz").unwrap();
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        assert_eq!(u64::from(decoder.header().unwrap().mtime()), timestamp);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert!(decoded.is_empty());
    }
    let timestamp = u64::from(u32::MAX) + 1;
    let tar = archive::encode_tar(timestamp, [("file", b"".as_slice())]).unwrap();
    let mut reader = tar::Archive::new(tar.as_slice());
    let entry = reader.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(entry.header().mtime().unwrap(), timestamp);
    assert!(matches!(
        archive::compress(&tar, timestamp, "tier0.tar.gz"),
        Err(Error::TimestampOutOfRange)
    ));
}
