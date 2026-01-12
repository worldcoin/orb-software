use color_eyre::eyre::{bail, Context, Result};
use object::{Object as _, ObjectSection as _};
use uuid::Uuid;

pub fn read_uuid_from_elf(data: &[u8]) -> Result<Uuid> {
    let file = object::File::parse(data)?;
    let mut data = None;
    for section in file.sections() {
        let Ok(name) = section.name() else {
            continue;
        };
        if name == ".ta_head" {
            data = Some(
                section
                    .uncompressed_data()
                    .wrap_err("failed to get section contets")?,
            );
            break;
        }
    }

    let Some(data) = data else {
        bail!("no .ta_head section found");
    };

    let data = &data[..16];
    let parsed_uuid = Uuid::from_slice_le(data).wrap_err("failed to get uuid")?;

    Ok(parsed_uuid)
}
