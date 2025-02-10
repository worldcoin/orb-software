use color_eyre::Result;
use minicbor::Encoder;

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut bytes = Vec::new();
    let mut encoder = minicbor::Encoder::new(&mut bytes);

    encoder
        .map(6)?
        .add_version_num(u8::MAX)?
        .add_sensor_data(u16::MAX, u16::MAX, true)?
        .add_sensor_data(u16::MAX, u16::MAX, true)?
        .add_sensor_data(u16::MAX, u16::MAX, true)?
        .add_sensor_data(u16::MAX, u16::MAX, true)?
        .add_sensor_data(u16::MAX, u16::MAX, true)?;

    println!("serialized payload size: {}", bytes.len());

    Ok(())
}

trait EncoderExt<W: minicbor::encode::Write> {
    fn add_sensor_data(
        &mut self,
        debounced_count: u16,
        raw_count: u16,
        debounced_state: bool,
    ) -> Result<&mut Encoder<W>, minicbor::encode::Error<W::Error>>;

    fn add_version_num(
        &mut self,
        version: u8,
    ) -> Result<&mut Encoder<W>, minicbor::encode::Error<W::Error>>;
}

impl<W: minicbor::encode::Write> EncoderExt<W> for Encoder<W> {
    fn add_sensor_data(
        &mut self,
        debounced_count: u16,
        raw_count: u16,
        debounced_state: bool,
    ) -> Result<&mut Encoder<W>, minicbor::encode::Error<W::Error>> {
        self.map(3)?
            .u8(0)?
            .u16(debounced_count)?
            .u8(1)?
            .u16(raw_count)?
            .u8(2)?
            .bool(debounced_state)
    }

    fn add_version_num(
        &mut self,
        version: u8,
    ) -> Result<
        &mut Encoder<W>,
        minicbor::encode::Error<<W as minicbor::encode::Write>::Error>,
    > {
        self.u8(0)?.u8(version)
    }
}
