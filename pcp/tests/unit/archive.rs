mod tar_gzip {
    use std::io::Read;

    use crate::archive::{self, ArchiveError};

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
        let bytes =
            archive::encode_tar(123, [("example.bin", b"abc".as_slice())]).unwrap();
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
            let bytes =
                archive::encode_tar(0, [(name.as_str(), b"".as_slice())]).unwrap();
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
                Err(ArchiveError::InvalidName)
            ));
            assert!(matches!(
                archive::compress(b"", 0, name),
                Err(ArchiveError::InvalidName)
            ));
        }
    }

    #[test]
    fn duplicate_filenames_are_rejected() {
        for second in [b"a".as_slice(), b"b".as_slice()] {
            assert!(matches!(
                archive::encode_tar(0, [("file", b"a".as_slice()), ("file", second)]),
                Err(ArchiveError::DuplicateName)
            ));
        }
    }

    #[test]
    fn gzip_preserves_metadata_and_round_trips_tar_bytes() {
        let tar =
            archive::encode_tar(123, [("example.bin", b"abc".as_slice())]).unwrap();
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
            Err(ArchiveError::TimestampOutOfRange)
        ));
    }
}

mod inner_archives {
    use std::{collections::BTreeMap, io::Read};

    use crate::{
        archive::{self, IrisEye, IrisFrame, NormalizedIrisFrame, PackageImages},
        crypto,
    };
    use rand::{rngs::StdRng, SeedableRng};
    use ring::digest::{digest, SHA256};

    fn frame(id: &str) -> IrisFrame<'_> {
        IrisFrame {
            image_id: id,
            ir_png: b"synthetic-png",
            normalized: Some(NormalizedIrisFrame {
                image: &[1; 512],
                mask: b"mask",
                image_resized: b"resized image",
                mask_resized: b"resized mask",
            }),
        }
    }

    fn images<'a>(
        left: &'a [IrisFrame<'a>],
        right: &'a [IrisFrame<'a>],
    ) -> PackageImages<'a> {
        PackageImages {
            left: Some(IrisEye {
                primary: frame("primary-left"),
                multiframe: left,
            }),
            right: Some(IrisEye {
                primary: frame("primary-right"),
                multiframe: right,
            }),
            thumbnail_png: None,
            face_ir_png: None,
            thermal_png: None,
            fraud: None,
        }
    }

    fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        tar::Archive::new(bytes)
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                assert_eq!(entry.header().mtime().unwrap(), 123);
                let name = entry.path().unwrap().to_str().unwrap().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                (name, data)
            })
            .collect()
    }

    #[test]
    fn archives_preserve_order_and_hash_every_file() {
        let left = [frame("extra-left-0"), frame("extra-left-1")];
        let right = [frame("extra-right")];
        let input = images(&left, &right);
        let encoded =
            archive::encode_inner(123, &input, &mut StdRng::seed_from_u64(3)).unwrap();
        let iris = entries(&encoded.iris);
        assert_eq!(
            iris.iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "left_ir.png",
                "right_ir.png",
                "extra-left-0.png",
                "extra-left-1.png",
                "extra-right.png"
            ]
        );

        let mut groups = vec![
            ("left", false, false),
            ("left", true, false),
            ("right", false, false),
            ("right", true, false),
            ("left", false, true),
            ("left", true, true),
            ("right", false, true),
            ("right", true, true),
        ];
        for id in ["extra-left-0", "extra-left-1", "extra-right"] {
            groups.extend([
                (id, false, false),
                (id, true, false),
                (id, false, true),
                (id, true, true),
            ]);
        }
        let mut expected = Vec::new();
        let mut rng = StdRng::seed_from_u64(3);
        for (id, mask, resized) in groups {
            let kind = if mask { "mask" } else { "image" };
            let suffix = if resized { "_resized" } else { "" };
            let data = match (mask, resized) {
                (false, false) => &[1; 512][..],
                (true, false) => b"mask",
                (false, true) => b"resized image",
                (true, true) => b"resized mask",
            };
            let generated = crypto::generate_commitment(data, &mut rng).unwrap();
            expected
                .push((format!("{id}_normalized_{kind}{suffix}.bin"), data.to_vec()));
            expected.push((
                format!("{id}_normalized_{kind}_commitment{suffix}.bin"),
                generated.commitment().to_vec(),
            ));
            expected.push((
                format!("{id}_normalized_{kind}_blinding_factors{suffix}.bin"),
                generated.blinding_factors().to_vec(),
            ));
        }
        assert_eq!(entries(&encoded.normalized_iris), expected);
        let mut hashes = BTreeMap::new();
        for bytes in [
            &encoded.iris,
            &encoded.normalized_iris,
            &encoded.face,
            &encoded.face_ir_and_thermal,
        ] {
            for (name, data) in entries(bytes) {
                let hash: [u8; 32] =
                    digest(&SHA256, &data).as_ref().try_into().unwrap();
                assert!(hashes.insert(name, hash).is_none());
            }
        }
        assert_eq!(encoded.hashes, hashes);
    }

    #[test]
    fn captured_multiframes_without_normalization_keep_images_but_add_no_normalized_files(
    ) {
        use rand::RngCore;

        let mut extra_left = frame("extra-left");
        extra_left.normalized = None;
        let mut extra_right = frame("extra-right");
        extra_right.normalized = None;
        let mut rng = StdRng::seed_from_u64(7);
        let mut primary_rng = rng.clone();
        let primary =
            archive::encode_inner(123, &images(&[], &[]), &mut primary_rng).unwrap();
        let encoded = archive::encode_inner(
            123,
            &images(&[extra_left], &[extra_right]),
            &mut rng,
        )
        .unwrap();

        assert_eq!(entries(&encoded.iris).len(), 4);
        assert_eq!(encoded.normalized_iris, primary.normalized_iris);
        assert_eq!(rng.next_u64(), primary_rng.next_u64());
        let mut expected_hashes = primary.hashes;
        for id in ["extra-left", "extra-right"] {
            let name = format!("{id}.png");
            assert!(entries(&encoded.iris)
                .contains(&(name.clone(), b"synthetic-png".to_vec())));
            expected_hashes.insert(
                name,
                digest(&SHA256, b"synthetic-png")
                    .as_ref()
                    .try_into()
                    .unwrap(),
            );
        }
        assert_eq!(encoded.hashes, expected_hashes);
    }

    #[test]
    fn missing_primary_normalization_fails_before_randomness() {
        use rand::RngCore;

        for left_missing in [true, false] {
            let mut input = images(&[], &[]);
            let eye = if left_missing {
                &mut input.left
            } else {
                &mut input.right
            };
            eye.as_mut().unwrap().primary.normalized = None;
            let mut rng = StdRng::seed_from_u64(0);
            let error = archive::encode_inner(123, &input, &mut rng).err().unwrap();
            assert!(matches!(
                (left_missing, error),
                (true, archive::InnerArchiveError::MissingLeftNormalization)
                    | (false, archive::InnerArchiveError::MissingRightNormalization)
            ));
            assert_eq!(rng.next_u64(), StdRng::seed_from_u64(0).next_u64());
        }
    }

    #[test]
    fn absent_thumbnail_and_modalities_keep_legacy_empty_entries() {
        let encoded = archive::encode_inner(
            123,
            &images(&[], &[]),
            &mut StdRng::seed_from_u64(0),
        )
        .unwrap();
        assert_eq!(
            entries(&encoded.face),
            vec![("thumbnail.png".into(), vec![])]
        );
        assert_eq!(encoded.face_ir_and_thermal, vec![0; 1024]);
        assert!(encoded.fraud.is_none());
    }

    #[test]
    fn fraud_and_modalities_keep_fixed_order_and_skip_absent_images() {
        let mut input = images(&[], &[]);
        input.thumbnail_png = Some(b"thumbnail");
        input.face_ir_png = Some(b"face ir");
        input.thermal_png = Some(b"thermal");
        input.fraud = Some(archive::FraudImages {
            scc_rgb_png: b"scc",
            left_rgb_png: b"left",
            right_rgb_png: b"right",
            left_thermal_png: None,
            right_thermal_png: Some(b"right thermal"),
            scc_depth_png: Some(b"scc depth"),
            left_depth_png: None,
            right_depth_png: Some(b"right depth"),
        });
        let encoded =
            archive::encode_inner(123, &input, &mut StdRng::seed_from_u64(0)).unwrap();
        let fraud = entries(encoded.fraud.as_ref().unwrap());
        assert_eq!(
            fraud
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                "scc_rgb.png",
                "left_rgb.png",
                "right_rgb.png",
                "right_thermal.png",
                "scc_depth.png",
                "right_depth.png"
            ]
        );
        assert_eq!(
            entries(&encoded.face_ir_and_thermal),
            vec![
                ("face_ir.png".into(), b"face ir".to_vec()),
                ("thermal.png".into(), b"thermal".to_vec())
            ]
        );
        for (name, data) in fraud {
            assert_eq!(
                encoded.hashes[&name].as_slice(),
                digest(&SHA256, &data).as_ref()
            );
        }
    }

    #[test]
    fn missing_eyes_fail_without_consuming_randomness() {
        use rand::RngCore;
        for left_missing in [true, false] {
            let mut input = images(&[], &[]);
            if left_missing {
                input.left = None;
            } else {
                input.right = None;
            }
            let mut rng = StdRng::seed_from_u64(0);
            let error = archive::encode_inner(123, &input, &mut rng).err().unwrap();
            assert!(matches!(
                (left_missing, error),
                (true, archive::InnerArchiveError::MissingLeftEye)
                    | (false, archive::InnerArchiveError::MissingRightEye)
            ));
            assert_eq!(rng.next_u64(), StdRng::seed_from_u64(0).next_u64());
        }
    }

    #[test]
    fn duplicate_and_unsafe_derived_names_are_rejected() {
        for id in ["left_ir", "left", "thumbnail", "../escape"] {
            let extra = [frame(id)];
            assert!(archive::encode_inner(
                123,
                &images(&extra, &[]),
                &mut StdRng::seed_from_u64(0)
            )
            .is_err());
        }
        let left = [frame("same")];
        let right = [frame("same")];
        assert!(archive::encode_inner(
            123,
            &images(&left, &right),
            &mut StdRng::seed_from_u64(0)
        )
        .is_err());
    }
}

mod tier_layout {
    use std::io::Read;

    use crate::archive::{
        self, BiometricArchives, PreparedBiometricFiles, Tier0Files, TierFormat,
    };

    fn biometrics(fraud: bool) -> PreparedBiometricFiles<'static> {
        PreparedBiometricFiles {
            archives: BiometricArchives {
                iris_sealed: b"iris-ciphertext",
                normalized_iris_sealed: b"normalized-ciphertext",
                face_sealed: b"face-ciphertext",
                fraud_sealed: fraud.then_some(b"fraud-ciphertext".as_slice()),
                face_ir_and_thermal: b"modality-archive",
            },
            face_embeddings_json: b"face-json",
            iris_codes_json: b"iris-json",
            iris_code_shares_json: [b"iris-share-0", b"iris-share-1", b"iris-share-2"],
            di_iris_embeddings_pb: b"di-protobuf",
            di_iris_embeddings_shares_pb: [b"di-share-0", b"di-share-1", b"di-share-2"],
        }
    }

    fn common_files() -> Tier0Files<'static> {
        Tier0Files {
            info_json: b"info-json",
            hashes_json: b"hashes-json",
            hashes_signature: b"signature",
            backend_keys_json: b"backend-keys-json",
        }
    }

    fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        tar::Archive::new(bytes)
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                assert_eq!(entry.header().mtime().unwrap(), 123);
                let name = entry.path().unwrap().to_str().unwrap().to_owned();
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                (name, data)
            })
            .collect()
    }

    fn assert_entries(actual: &[u8], expected: &[(&str, &[u8])]) {
        let expected: Vec<_> = expected
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect();
        assert_eq!(entries(actual), expected);
    }

    #[test]
    fn v2_places_all_archives_in_tier0_before_payloads() {
        for fraud in [false, true] {
            let bio = biometrics(fraud);
            let auxiliary =
                archive::auxiliary_tiers(TierFormat::V2, 123, Some(&bio)).unwrap();
            assert_eq!(auxiliary.tier1, vec![0; 1024]);
            assert_eq!(auxiliary.tier2, vec![0; 1024]);
            let bytes = archive::tier0(TierFormat::V2, 123, common_files(), Some(&bio))
                .unwrap();
            let mut expected: Vec<(&str, &[u8])> = vec![
                ("iris.tar", b"iris-ciphertext"),
                ("normalized_iris.tar", b"normalized-ciphertext"),
                ("face.tar", b"face-ciphertext"),
            ];
            if fraud {
                expected.push(("fraud.tar", b"fraud-ciphertext"));
            }
            expected.push(("face_ir_and_thermal.tar", b"modality-archive"));
            expected.extend(full_tier0_remainder());
            assert_entries(&bytes, &expected);
        }
    }

    #[test]
    fn v3_routes_archives_to_auxiliary_tiers_without_changing_bytes() {
        for fraud in [false, true] {
            let bio = biometrics(fraud);
            let auxiliary =
                archive::auxiliary_tiers(TierFormat::V3, 123, Some(&bio)).unwrap();
            let mut expected: Vec<(&str, &[u8])> = vec![
                ("iris.tar", b"iris-ciphertext"),
                ("normalized_iris.tar", b"normalized-ciphertext"),
                ("face.tar", b"face-ciphertext"),
            ];
            if fraud {
                expected.push(("fraud.tar", b"fraud-ciphertext"));
            }
            assert_entries(&auxiliary.tier1, &expected);
            assert_entries(
                &auxiliary.tier2,
                &[("face_ir_and_thermal.tar", b"modality-archive")],
            );
            let tier0 = archive::tier0(TierFormat::V3, 123, common_files(), Some(&bio))
                .unwrap();
            assert_entries(&tier0, &full_tier0_remainder());
        }
    }

    #[test]
    fn omitted_biometrics_leave_only_four_tier0_files_and_empty_auxiliary_tiers() {
        for format in [TierFormat::V2, TierFormat::V3] {
            let auxiliary = archive::auxiliary_tiers(format, 123, None).unwrap();
            assert_eq!(auxiliary.tier1, vec![0; 1024]);
            assert_eq!(auxiliary.tier2, vec![0; 1024]);
            let tier0 = archive::tier0(format, 123, common_files(), None).unwrap();
            assert_entries(
                &tier0,
                &[
                    ("info.json", b"info-json"),
                    ("hashes.sign", b"signature"),
                    ("hashes.json", b"hashes-json"),
                    ("backend_keys.json", b"backend-keys-json"),
                ],
            );
        }
    }

    #[test]
    fn empty_di_and_share_payloads_are_present_not_omitted() {
        for format in [TierFormat::V2, TierFormat::V3] {
            let mut bio = biometrics(false);
            bio.di_iris_embeddings_pb = b"";
            bio.di_iris_embeddings_shares_pb = [b""; 3];
            let tier0 =
                archive::tier0(format, 123, common_files(), Some(&bio)).unwrap();
            let entries = entries(&tier0);
            let di_files: Vec<_> = entries
                .iter()
                .filter(|(name, _)| name.starts_with("di_iris_embeddings"))
                .collect();
            assert_eq!(di_files.len(), 4);
            assert!(di_files.iter().all(|(_, bytes)| bytes.is_empty()));
        }
    }

    fn full_tier0_remainder() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("info.json", b"info-json"),
            ("face_embeddings.json", b"face-json"),
            ("iris_codes.json", b"iris-json"),
            ("iris_code_shares_0.json", b"iris-share-0"),
            ("iris_code_shares_1.json", b"iris-share-1"),
            ("iris_code_shares_2.json", b"iris-share-2"),
            ("di_iris_embeddings.pb", b"di-protobuf"),
            ("di_iris_embeddings_shares_0.pb", b"di-share-0"),
            ("di_iris_embeddings_shares_1.pb", b"di-share-1"),
            ("di_iris_embeddings_shares_2.pb", b"di-share-2"),
            ("hashes.sign", b"signature"),
            ("hashes.json", b"hashes-json"),
            ("backend_keys.json", b"backend-keys-json"),
        ]
    }
}
