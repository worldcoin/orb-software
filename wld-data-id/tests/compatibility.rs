use orb_wld_data_id::{ImageId, S3Region, SignupId};
use serde_json::json;
use uuid::Uuid;

const SIGNUP: &str = "00120011223344556677889900000000";
const IMAGE: &str = "00120011223344556677889978563412";

#[test]
fn s3_region_names_and_wire_codes() {
    let regions = [
        (S3Region::AfSouth1, "af-south-1", 0),
        (S3Region::ApEast1, "ap-east-1", 1),
        (S3Region::ApNortheast1, "ap-northeast-1", 2),
        (S3Region::ApNortheast2, "ap-northeast-2", 3),
        (S3Region::ApNortheast3, "ap-northeast-3", 4),
        (S3Region::ApSouth1, "ap-south-1", 5),
        (S3Region::ApSoutheast1, "ap-southeast-1", 6),
        (S3Region::ApSoutheast2, "ap-southeast-2", 7),
        (S3Region::CaCentral1, "ca-central-1", 8),
        (S3Region::CnNorthwest1, "cn-northwest-1", 9),
        (S3Region::EuCentral1, "eu-central-1", 10),
        (S3Region::EuNorth1, "eu-north-1", 11),
        (S3Region::EuSouth1, "eu-south-1", 12),
        (S3Region::EuWest1, "eu-west-1", 13),
        (S3Region::EuWest2, "eu-west-2", 14),
        (S3Region::EuWest3, "eu-west-3", 15),
        (S3Region::MeSouth1, "me-south-1", 16),
        (S3Region::SaEast1, "sa-east-1", 17),
        (S3Region::UsEast1, "us-east-1", 18),
        (S3Region::UsEast2, "us-east-2", 19),
        (S3Region::UsGovEast1, "us-gov-east-1", 20),
        (S3Region::UsGovWest1, "us-gov-west-1", 21),
        (S3Region::UsWest1, "us-west-1", 22),
        (S3Region::UsWest2, "us-west-2", 23),
    ];
    for (region, name, code) in regions {
        assert_eq!(name.parse::<S3Region>().unwrap(), region);
        assert_eq!(region as u8, code);
        assert_eq!(serde_json::to_value(region).unwrap(), json!(code));
        assert_eq!(
            serde_json::from_value::<S3Region>(json!(code)).unwrap(),
            region
        );
    }

    for name in ["", "unknown", "US-EAST-1", "us-east-1 "] {
        assert_eq!(name.parse::<S3Region>().unwrap(), S3Region::Unknown);
    }
    for code in 24..=u8::MAX {
        assert_eq!(
            serde_json::from_value::<S3Region>(json!(code)).unwrap(),
            S3Region::Unknown,
        );
    }
    assert_eq!(serde_json::to_value(S3Region::Unknown).unwrap(), json!(255));
}

#[test]
fn signup_and_image_wire_format() {
    let signup: SignupId = SIGNUP.parse().unwrap();
    assert_eq!(signup.to_string(), SIGNUP);
    let image = ImageId::new(&signup, 0x12345678);
    assert_eq!(image.to_string(), IMAGE);
    assert_eq!(IMAGE.parse::<ImageId>().unwrap(), image);
    assert_eq!(SignupId::from(image), signup);
    assert_eq!(ImageId::new(&signup, 0).to_string(), SIGNUP);

    let hyphenated: SignupId = "00120011-2233-4455-6677-889900000000".parse().unwrap();
    assert_eq!(hyphenated, signup);
    let uppercase: SignupId = "00FF0011223344556677889900000000".parse().unwrap();
    assert_eq!(uppercase.to_string(), "00ff0011223344556677889900000000");
}

#[test]
fn serde_json_and_wire_format() {
    let signup: SignupId = SIGNUP.parse().unwrap();
    let expected = json!({
        "version": 0,
        "s3_region": 18,
        "signup_id": [0, 17, 34, 51, 68, 85, 102, 119, 136, 153],
        "data_id": 0,
    });
    assert_eq!(serde_json::to_value(&signup).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<SignupId>(expected).unwrap(),
        signup
    );

    let image = ImageId::new(&signup, 0x12345678);
    let bytes = [
        0x00, 0x12, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x78,
        0x56, 0x34, 0x12,
    ];
    assert_eq!(
        Uuid::parse_str(&image.to_string()).unwrap().as_bytes(),
        &bytes
    );
    assert_eq!(
        Uuid::from_bytes(bytes)
            .simple()
            .to_string()
            .parse::<ImageId>()
            .unwrap(),
        image
    );
}

#[test]
fn wire_format_preserves_fields_and_normalizes_unknown_regions() {
    for version in [0, 1, u8::MAX] {
        for region in 0..=u8::MAX {
            for data_id in [0, 0x12345678, u32::MAX] {
                let mut bytes = [0; 16];
                bytes[0] = version;
                bytes[1] = region;
                bytes[2..12]
                    .copy_from_slice(&[0, 17, 34, 51, 68, 85, 102, 119, 136, 153]);
                bytes[12..16].copy_from_slice(&data_id.to_le_bytes());

                let encoded = Uuid::from_bytes(bytes).simple().to_string();
                let parsed: ImageId = encoded.parse().unwrap();
                let expected_region = if region < 24 { region } else { u8::MAX };
                assert_eq!(
                    serde_json::to_value(&parsed).unwrap(),
                    json!({
                        "version": version,
                        "s3_region": expected_region,
                        "signup_id": [0, 17, 34, 51, 68, 85, 102, 119, 136, 153],
                        "data_id": data_id,
                    }),
                );
                bytes[1] = expected_region;
                assert_eq!(
                    parsed.to_string(),
                    Uuid::from_bytes(bytes).simple().to_string(),
                );
            }
        }
    }
}

#[test]
fn pin_format() {
    let signup: SignupId = SIGNUP.parse().unwrap();
    assert_eq!(signup.to_pin_string(), "665416");
}
