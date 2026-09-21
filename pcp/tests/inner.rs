use std::{collections::BTreeMap, io::Read};

use orb_pcp::{
    commitment,
    inner::{self, Eye, Frame, Images, NormalizedFrame},
};
use rand::{rngs::StdRng, SeedableRng};
use ring::digest::{digest, SHA256};

fn frame(id: &str) -> Frame<'_> {
    Frame {
        image_id: id,
        ir_png: b"synthetic-png",
        normalized: NormalizedFrame {
            image: &[1; 512],
            mask: b"mask",
            image_resized: b"resized image",
            mask_resized: b"resized mask",
        },
    }
}

fn images<'a>(left: &'a [Frame<'a>], right: &'a [Frame<'a>]) -> Images<'a> {
    Images {
        left: Some(Eye {
            primary: frame("primary-left"),
            multiframe: left,
        }),
        right: Some(Eye {
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
    let encoded = inner::encode(123, &input, &mut StdRng::seed_from_u64(3)).unwrap();
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
        let generated = commitment::generate(data, &mut rng).unwrap();
        expected.push((format!("{id}_normalized_{kind}{suffix}.bin"), data.to_vec()));
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
            let hash: [u8; 32] = digest(&SHA256, &data).as_ref().try_into().unwrap();
            assert!(hashes.insert(name, hash).is_none());
        }
    }
    assert_eq!(encoded.hashes, hashes);
}

#[test]
fn absent_thumbnail_and_modalities_keep_legacy_empty_entries() {
    let encoded =
        inner::encode(123, &images(&[], &[]), &mut StdRng::seed_from_u64(0)).unwrap();
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
    input.fraud = Some(inner::Fraud {
        scc_rgb_png: b"scc",
        left_rgb_png: b"left",
        right_rgb_png: b"right",
        left_thermal_png: None,
        right_thermal_png: Some(b"right thermal"),
        scc_depth_png: Some(b"scc depth"),
        left_depth_png: None,
        right_depth_png: Some(b"right depth"),
    });
    let encoded = inner::encode(123, &input, &mut StdRng::seed_from_u64(0)).unwrap();
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
        let error = inner::encode(123, &input, &mut rng).err().unwrap();
        assert!(matches!(
            (left_missing, error),
            (true, inner::Error::MissingLeftEye)
                | (false, inner::Error::MissingRightEye)
        ));
        assert_eq!(rng.next_u64(), StdRng::seed_from_u64(0).next_u64());
    }
}

#[test]
fn duplicate_and_unsafe_derived_names_are_rejected() {
    for id in ["left_ir", "left", "thumbnail", "../escape"] {
        let extra = [frame(id)];
        assert!(inner::encode(
            123,
            &images(&extra, &[]),
            &mut StdRng::seed_from_u64(0)
        )
        .is_err());
    }
    let left = [frame("same")];
    let right = [frame("same")];
    assert!(
        inner::encode(123, &images(&left, &right), &mut StdRng::seed_from_u64(0))
            .is_err()
    );
}
