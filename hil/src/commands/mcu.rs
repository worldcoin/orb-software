//! All registers come from the [RM0440][RM0440] reference manual for the STM32G4
//! series.
//!
//! Note: the reference manual divides chips up into product categories. the STM32G474
//! series are "category 3". See §1.5 of the manual.
//!
//! [RM0440]: https://www.st.com/resource/en/reference_manual/rm0440-stm32g4-series-advanced-armbased-32bit-mcus-stmicroelectronics.pdf

use std::{path::PathBuf, sync::Arc, time::Duration};

use color_eyre::{
    eyre::{bail, ensure, eyre, WrapErr as _},
    Result, Section as _,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use probe_rs::{
    flashing::{
        download_file_with_options, DownloadOptions, FlashProgress, Format,
        ProgressEvent, ProgressOperation,
    },
    probe::{Probe, WireProtocol},
    Core, MemoryInterface, Permissions, Session,
};
use tracing::{debug, info, warn};

// From probe-rs
const TARGET_NAME: &str = "STM32G474VETx";

/// Memory address of a block of registers.
struct RegBlock(u64);

/// A register.
trait Reg {
    const BLOCK: RegBlock;
    const OFFSET: u8;
    const ADDR: u64 = Self::BLOCK.0 + Self::OFFSET as u64;
}

trait Read<WORD> {
    fn read(memory: &mut probe_rs::Core<'_>) -> Result<WORD, probe_rs::Error>;
}

trait Write<WORD>: Read<WORD> {
    fn write(
        memory: &mut probe_rs::Core<'_>,
        word: WORD,
    ) -> Result<(), probe_rs::Error>;
}

impl<T: Reg> Read<u32> for T {
    fn read(memory: &mut probe_rs::Core<'_>) -> Result<u32, probe_rs::Error> {
        memory.read_word_32(Self::ADDR)
    }
}

impl<T: Reg> Write<u32> for T {
    fn write(
        memory: &mut probe_rs::Core<'_>,
        word: u32,
    ) -> Result<(), probe_rs::Error> {
        memory.write_word_32(Self::ADDR, word)
    }
}

/// `FLASH` register block. See §3.7.19 and §2.2.2
const FLASH_REG_BLOCK: RegBlock = RegBlock(0x4002_2000);

/// `FLASH_KEYR` register.
///
/// See §3.7.3
struct FlashKeyr;
impl Reg for FlashKeyr {
    const BLOCK: RegBlock = FLASH_REG_BLOCK;
    const OFFSET: u8 = 0x08;
}

/// `FLASH_OPTKEYR` register.
///
/// See §3.7.4
struct FlashOptkeyr;
impl Reg for FlashOptkeyr {
    const BLOCK: RegBlock = FLASH_REG_BLOCK;
    const OFFSET: u8 = 0x0C;
}

/// `FLASH_SR` register.
///
/// See §3.7.5
struct FlashSr;
impl Reg for FlashSr {
    const BLOCK: RegBlock = FLASH_REG_BLOCK;
    const OFFSET: u8 = 0x10;
}
impl FlashSr {
    const BSY_BIT: u8 = 16;
    /// Whether `FLASH_SR_BSY` is high. This indicates if a flash memory operation
    /// is in progress.
    fn is_bsy(core: &mut Core) -> Result<bool> {
        let flash_sr = Self::read(core).wrap_err("FLASH_SR read failed")?;

        Ok((flash_sr & (1u32 << Self::BSY_BIT)) != 0)
    }
}

/// `FLASH_CR` register.
///
/// See §3.7.6
struct FlashCr;
impl Reg for FlashCr {
    const BLOCK: RegBlock = FLASH_REG_BLOCK;
    const OFFSET: u8 = 0x14;
}
impl FlashCr {
    const LOCK_BIT: u8 = 31;
    const OPTLOCK_BIT: u8 = 30;
    const OBL_LAUNCH_BIT: u8 = 27;
    const OPTSTRT_BIT: u8 = 17;

    fn is_lock(core: &mut Core) -> Result<bool> {
        let flash_cr = FlashCr::read(core).wrap_err("FLASH_CR read failed")?;

        Ok((flash_cr & (1u32 << Self::LOCK_BIT)) != 0)
    }

    fn is_optlock(core: &mut Core) -> Result<bool> {
        let flash_cr = FlashCr::read(core).wrap_err("FLASH_CR read failed")?;

        Ok((flash_cr & (1u32 << Self::OPTLOCK_BIT)) != 0)
    }

    /// Unlock sequence for `FLASH_CR_LOCK`.
    ///
    /// See §3.3.5 of [RM0440][RM0440]
    fn clear_lock(core: &mut Core) -> Result<()> {
        ensure!(Self::is_lock(core)?, "FLASH_CR_LOCK already cleared");

        // unlock LOCK via FLASH_KEYR sequence
        // See
        FlashKeyr::write(core, 0x4567_0123)
            .and_then(|()| FlashKeyr::write(core, 0xCDEF_89AB))
            .wrap_err("FLASH_KEYR write failed")?;

        ensure!(!Self::is_lock(core)?, "FLASH_CR_LOCK failed to clear!");

        Ok(())
    }

    /// Unlock sequence for `FLASH_CR_OPTLOCK`. Note that `FLASH_CR_LOCK` must first
    /// be unlocked.
    ///
    /// See §3.4.2 of [RM0440][RM0440]
    fn clear_optlock(core: &mut Core) -> Result<()> {
        ensure!(!Self::is_lock(core)?, "FLASH_CR_LOCK must be cleared first");
        ensure!(Self::is_optlock(core)?, "FLASH_CR_OPTLOCK already cleared");

        // unlock OPTLOCK via FLASH_OPTKEYR sequence
        FlashOptkeyr::write(core, 0x08192A3B)
            .and_then(|()| FlashOptkeyr::write(core, 0x4C5D6E7F))
            .wrap_err("FLASH_OPTKEYR write failed")?;

        ensure!(
            !Self::is_optlock(core)?,
            "FLASH_CR_OPTLOCK failed to clear!"
        );

        Ok(())
    }

    /// Triggers the `FLASH_CR_OPT_STRT` bit, and waits for `FLASH_SR_BSY` clear.
    ///
    /// See §3.4.2
    fn trigger_optstrt(core: &mut Core) -> Result<()> {
        ensure!(
            !Self::is_optlock(core)?,
            "FLASH_CR_OPTLOCK must be cleared first"
        );

        let flash_cr = FlashCr::read(core).wrap_err("FLASH_CR read failed")?;
        let new_flash_cr = flash_cr | (1u32 << Self::OPTSTRT_BIT);
        FlashCr::write(core, new_flash_cr).wrap_err("FLASH_CR write failed")?;

        // loop until FLASH_SR_BSY bit is complete
        let start_time = std::time::Instant::now();
        while start_time.elapsed() < Duration::from_millis(1000) {
            std::thread::sleep(Duration::from_millis(50));
            if !FlashSr::is_bsy(core).wrap_err("FLASH_SR_BSY read failed")? {
                // completed
                return Ok(());
            }
        }
        bail!("timed out waiting for FLASH_SR_BSY to complete");
    }

    /// `OBL_LAUNCH` bit in `FLASH_CR`. Must be called to trigger a reload for option
    /// bytes. It will also reset the device.
    fn trigger_obl_launch(core: &mut Core) -> Result<()> {
        ensure!(
            !Self::is_optlock(core)?,
            "FLASH_CR_OPTLOCK must be cleared first"
        );
        let flash_cr = FlashCr::read(core).wrap_err("FLASH_CR read failed")?;
        let new_flash_cr = flash_cr | (1u32 << Self::OBL_LAUNCH_BIT);
        FlashCr::write(core, new_flash_cr).wrap_err("FLASH_CR write failed")?;

        // loop until launch bit is complete
        let start_time = std::time::Instant::now();
        while start_time.elapsed() < Duration::from_millis(1000) {
            std::thread::sleep(Duration::from_millis(50));
            let flash_cr = FlashCr::read(core).wrap_err("FLASH_CR read failed")?;
            if flash_cr & (1u32 << Self::OBL_LAUNCH_BIT) == 0 {
                // completed
                return Ok(());
            }
        }
        bail!("timed out waiting for FLASH_CR_OBL_LAUNCH to complete");
    }
}

/// `FLASH_OPTR` register.
///
/// See §3.7.8
struct FlashOptr;
impl Reg for FlashOptr {
    const BLOCK: RegBlock = FLASH_REG_BLOCK;
    const OFFSET: u8 = 0x20;
}
impl FlashOptr {
    fn read_rdp(core: &mut Core) -> Result<RdpLevel> {
        let flash_optr = Self::read(core).wrap_err("FLASH_OPTR read failed")?;
        let rdp = flash_optr & 0xFF;
        ensure!(
            rdp != 0xCC,
            "currently set to Level 2 RDP which means you lost debugging capability, \
            get a new MCU"
        );

        if rdp == RdpLevel::L0 as u32 {
            Ok(RdpLevel::L0)
        } else {
            Ok(RdpLevel::L1)
        }
    }

    fn write_rdp(core: &mut Core, level: RdpLevel) -> Result<()> {
        ensure!(
            !FlashCr::is_lock(core)? && !FlashCr::is_optlock(core)?,
            "can only write RDP whe FLASH_CR_LOCK and FLASH_CR_OPTLOCK are cleared"
        );
        let flash_optr = FlashOptr::read(core).wrap_err("FLASH_OPTR read failed")?;
        // write to FLASH_OPTR to control readout protection
        let new_flash_optr = (flash_optr & !0xFF) | level as u32;
        debug!("writing optr: {new_flash_optr:0X}");
        FlashOptr::write(core, new_flash_optr).wrap_err("FLASH_OPTR write failed")
    }
}

/// the different values for `FLASH_OPTR_RDP`.
///
/// See §3.4.1
#[derive(clap::ValueEnum, Debug, Eq, PartialEq, Copy, Clone, Default)]
#[repr(u8)]
enum RdpLevel {
    #[default]
    L0 = 0xAA,
    L1 = 0xBB,
    // L2 = 0xCC // commented out because setting this will prevent further debugging.
}

#[derive(Debug, clap::Parser)]
pub struct Mcu {
    #[clap(subcommand)]
    subcommands: Subcommands,
}

impl Mcu {
    pub async fn run(self) -> Result<()> {
        self.subcommands.run().await
    }
}

/// Microcontrollers utilities (flash, read protection)
#[derive(Debug, clap::Parser)]
enum Subcommands {
    Flash(FlashCommand),
    Rdp(RdpCommand),
}

impl Subcommands {
    async fn run(self) -> Result<()> {
        match self {
            Subcommands::Flash(c) => c.run().await,
            Subcommands::Rdp(c) => c.run().await,
        }
    }
}

/// Flash firmware to the MCU.
///
/// Requires a hardware debugger/probe.
#[derive(Debug, clap::Parser)]
struct FlashCommand {
    /// Path to the hex file to flash
    #[clap(long)]
    file: PathBuf,
    /// The USB serial number of the probe to use
    #[clap(long)]
    serial: Option<String>,
    /// vendor_id:[product_id]
    #[clap(long, value_parser = usb_device_parser)]
    device: Option<(u16, u16)>,
}

impl FlashCommand {
    async fn run(self) -> Result<()> {
        tokio::task::spawn_blocking(|| self.run_blocking())
            .await
            .expect("task panicked")
    }

    fn run_blocking(self) -> Result<()> {
        ensure!(
            self.file.exists(),
            "hex file does not exist: {}",
            self.file.display()
        );

        let filter = ProbeFilter::new(self.serial, self.device);

        let probe = get_probe(&filter).wrap_err("failed to get a hardware probe")?;
        let mut session =
            attach_probe(probe, true).wrap_err("failed to attach probe to mcu")?;
        info!("attached to mcu!");

        // Check RDP level before flashing
        let mut core = session.core(0).wrap_err("failed to get core 0")?;
        let rdp_level = FlashOptr::read_rdp(&mut core);
        drop(core);

        let mut session = match rdp_level {
            Ok(RdpLevel::L0) => {
                info!("RDP level is L0 (no protection)");
                session
            }
            Ok(RdpLevel::L1) => {
                warn!(
                    "RDP level is L1 (read-out protection enabled). \
                    Removing protection before flashing..."
                );
                drop(session);
                set_rdp_level(&filter, RdpLevel::L0)
                    .wrap_err("failed to remove read-out protection")?;
                info!("read-out protection removed successfully");

                // Re-attach after RDP change
                let probe =
                    get_probe(&filter).wrap_err("failed to get a hardware probe")?;
                attach_probe(probe, true).wrap_err("failed to attach probe to mcu")?
            }
            Err(e) => {
                warn!("failed to read RDP level: {e}");
                session
            }
        };

        info!("flashing file: {}", self.file.display());
        let progress = FlashProgressBar::new();
        let mut options = DownloadOptions::default();
        options.progress = FlashProgress::new(progress.callback());
        options.verify = true;
        download_file_with_options(&mut session, &self.file, Format::Hex, options)
            .wrap_err("failed to flash hex file")?;

        info!("resetting target...");
        let mut core = session.core(0).wrap_err("failed to get core 0")?;
        core.reset().wrap_err("failed to reset target")?;

        info!("flash completed successfully!");

        Ok(())
    }
}

/// Control read-out protection.
///
/// Requires a hardware debugger/probe.
#[derive(Debug, clap::Parser)]
struct RdpCommand {
    /// The USB serial number of the probe to use
    #[clap(long)]
    serial: Option<String>,
    /// vendor_id:[product_id]
    #[clap(long, value_parser = usb_device_parser)]
    device: Option<(u16, u16)>,
    #[clap(long)]
    protect: bool,
}

fn usb_device_parser(s: &str) -> Result<(u16, u16)> {
    let (vid, pid) = match s.split_once(':') {
        Some((vid, pid)) => {
            let pid = u16::from_str_radix(pid, 16)
                .wrap_err("expected base16 for productid")?;
            (vid, pid)
        }
        None => (s, 0),
    };
    let vid = u16::from_str_radix(vid, 16).wrap_err("expected base16 for vendorid")?;
    Ok((vid, pid))
}

impl RdpCommand {
    async fn run(self) -> Result<()> {
        tokio::task::spawn_blocking(|| self.run_blocking())
            .await
            .expect("task panicked")
    }

    fn run_blocking(self) -> Result<()> {
        let target_rdp = if self.protect {
            RdpLevel::L1
        } else {
            RdpLevel::L0
        };
        let filter = ProbeFilter::new(self.serial, self.device);
        set_rdp_level(&filter, target_rdp)
    }
}

fn attach_probe(mut probe: Probe, allow_erase: bool) -> Result<Session> {
    // SWD is mandatory (JTAG is not supported by microcontrollers on Orbs)
    probe
        .select_protocol(WireProtocol::Swd)
        .wrap_err("failed to select swd protocol")
        .note("orbs prefer SWD debug protocol")?;

    let perms = if allow_erase {
        Permissions::new().allow_erase_all()
    } else {
        Permissions::new()
    };

    probe
        .attach(TARGET_NAME, perms)
        .wrap_err_with(|| format!("failed to attach to {TARGET_NAME}"))
}

/// Progress bar wrapper for flash operations.
struct FlashProgressBar {
    #[allow(dead_code)] // Kept alive to maintain the multi-progress display
    multi: Arc<MultiProgress>,
    erase_bar: Arc<ProgressBar>,
    program_bar: Arc<ProgressBar>,
    verify_bar: Arc<ProgressBar>,
}

impl FlashProgressBar {
    fn new() -> Self {
        let multi = MultiProgress::new();

        let erase_bar = multi.add(ProgressBar::new(0));
        erase_bar.set_style(Self::erase_style());

        let program_bar = multi.add(ProgressBar::new(0));
        program_bar.set_style(Self::program_style());

        let verify_bar = multi.add(ProgressBar::new(0));
        verify_bar.set_style(Self::verify_style());

        Self {
            multi: Arc::new(multi),
            erase_bar: Arc::new(erase_bar),
            program_bar: Arc::new(program_bar),
            verify_bar: Arc::new(verify_bar),
        }
    }

    fn erase_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} {msg:12} [{bar:40.yellow/red}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("valid template")
            .progress_chars("█▓░")
    }

    fn program_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} {msg:12} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("valid template")
            .progress_chars("█▓░")
    }

    fn verify_style() -> ProgressStyle {
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} {msg:12} [{bar:40.green/white}] {bytes}/{total_bytes} ({eta})",
            )
            .expect("valid template")
            .progress_chars("█▓░")
    }

    fn callback(&self) -> impl FnMut(ProgressEvent) + Send + Sync + 'static {
        let erase_bar = Arc::clone(&self.erase_bar);
        let program_bar = Arc::clone(&self.program_bar);
        let verify_bar = Arc::clone(&self.verify_bar);
        move |event| match event {
            ProgressEvent::AddProgressBar { operation, total } => {
                let bar = match operation {
                    ProgressOperation::Erase => &erase_bar,
                    ProgressOperation::Program => &program_bar,
                    ProgressOperation::Verify => &verify_bar,
                    ProgressOperation::Fill => return,
                };
                bar.set_length(total.unwrap_or(0));
                bar.set_position(0);
            }
            ProgressEvent::Started(operation) => {
                let (bar, msg) = match operation {
                    ProgressOperation::Erase => (&erase_bar, "Erasing"),
                    ProgressOperation::Program => (&program_bar, "Programming"),
                    ProgressOperation::Verify => (&verify_bar, "Verifying"),
                    ProgressOperation::Fill => return,
                };
                bar.set_message(msg);
            }
            ProgressEvent::Progress {
                operation, size, ..
            } => {
                let bar = match operation {
                    ProgressOperation::Erase => &erase_bar,
                    ProgressOperation::Program => &program_bar,
                    ProgressOperation::Verify => &verify_bar,
                    ProgressOperation::Fill => return,
                };
                bar.inc(size);
            }
            ProgressEvent::Finished(operation) => {
                let (bar, msg) = match operation {
                    ProgressOperation::Erase => (&erase_bar, "Erased"),
                    ProgressOperation::Program => (&program_bar, "Programmed"),
                    ProgressOperation::Verify => (&verify_bar, "Verified"),
                    ProgressOperation::Fill => return,
                };
                bar.finish_with_message(msg);
            }
            ProgressEvent::FlashLayoutReady { .. }
            | ProgressEvent::Failed(_)
            | ProgressEvent::DiagnosticMessage { .. } => {}
        }
    }
}

/// Filter options for selecting a debug probe.
#[derive(Debug, Clone, Default)]
struct ProbeFilter {
    serial: Option<String>,
    device: Option<(u16, u16)>,
}

impl ProbeFilter {
    fn new(serial: Option<String>, device: Option<(u16, u16)>) -> Self {
        Self { serial, device }
    }
}

fn get_probe(filter: &ProbeFilter) -> Result<Probe> {
    let lister = probe_rs::probe::list::Lister::new();
    let probes = lister.list_all();
    if probes.is_empty() {
        return Err(eyre!("no debug probes found"))
            .suggestion(
                "make sure a hardware probe/debugger is connected to your computer",
            )
            .suggestion(
                "make sure your udev rules are configured and you can read from usb",
            );
    }
    debug!("Found probes:");
    for p in probes.iter() {
        debug!("{p}");
    }

    // Filter probes based on command line arguments
    let filtered_probes: Vec<_> = probes
        .into_iter()
        .filter(|p| {
            filter
                .serial
                .as_ref()
                .map(|expected_serial| {
                    p.serial_number.as_deref().unwrap_or_default() == expected_serial
                })
                .unwrap_or(true)
        })
        .filter(|p| {
            filter
                .device
                .map(|(expected_vid, expected_pid)| {
                    p.vendor_id == expected_vid
                        && (p.product_id == expected_pid || expected_pid == 0)
                })
                .unwrap_or(true)
        })
        .collect();

    let probe = match filtered_probes.len() {
        0 => bail!("no probes match the provided filter criteria"),
        1 => filtered_probes.into_iter().next().unwrap(),
        _ => {
            // Multiple probes found, let the user select one
            let items: Vec<String> = filtered_probes
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}: {}", i, p))
                .collect();

            println!("Multiple probes found. Please select one:");
            let selection = dialoguer::Select::new()
                .items(&items)
                .default(0)
                .interact()
                .wrap_err("failed to get user selection")?;

            filtered_probes.into_iter().nth(selection).unwrap()
        }
    };

    info!("using probe {probe}");

    probe.open().wrap_err("failed to open probe")
}

/// Set the RDP level on the MCU.
///
/// This function handles getting the probe, setting RDP, triggering the reset,
/// and verifying the new RDP level.
fn set_rdp_level(filter: &ProbeFilter, target_rdp: RdpLevel) -> Result<()> {
    let probe = get_probe(filter).wrap_err("failed to get a hardware probe")?;
    let mut session =
        attach_probe(probe, false).wrap_err("failed to attach probe to mcu")?;
    let mut core = session.core(0).wrap_err("failed to get core 0")?;
    info!("attached to mcu for RDP change!");

    // read initial state
    let flash_cr = FlashCr::read(&mut core).wrap_err("FLASH_CR read failed")?;
    debug!("FLASH_CR before: {flash_cr:0X}");
    let flash_sr = FlashSr::read(&mut core).wrap_err("FLASH_SR read failed")?;
    debug!("FLASH_SR before: {flash_sr:0X}");
    let flash_optr = FlashOptr::read(&mut core).wrap_err("FLASH_OPTR read failed")?;
    debug!("FLASH_OPTR before: {flash_optr:0X}");

    let rdp = FlashOptr::read_rdp(&mut core).wrap_err("FLASH_OPTR_RDP read failed")?;
    info!("RDP is currently {rdp:?}");
    if rdp == target_rdp {
        warn!("RDP already matches desired setting, we are done");

        return Ok(());
    }

    ensure!(
        !FlashSr::is_bsy(&mut core)?,
        "we shouldn't have done anything with flash yet"
    );
    // Clear two locks guarding FLASH_OPTR (aka option register)
    debug!("clearing FLASH_LOCK");
    FlashCr::clear_lock(&mut core).wrap_err("failed to clear FLASH_CR_LOCK")?;
    debug!("clearing FLASH_OPTLOCK");
    FlashCr::clear_optlock(&mut core).wrap_err("failed to clear FLASH_CR_OPTLOCK")?;
    FlashOptr::write_rdp(&mut core, target_rdp)
        .wrap_err("FLASH_OPTR_RDP write failed")?;
    FlashCr::trigger_optstrt(&mut core)
        .wrap_err("FLASH_CR_OPTSTRT failed to trigger")?;
    // its expected for this one to fail, since it resets the device
    let _ = FlashCr::trigger_obl_launch(&mut core);
    drop(core);
    drop(session);

    let probe = get_probe(filter).wrap_err("failed to get a hardware probe")?;
    let mut session =
        attach_probe(probe, false).wrap_err("failed to attach probe to mcu")?;
    let mut core = session.core(0).wrap_err("failed to get core 0")?;
    info!("reattached to mcu!");

    let rdp = FlashOptr::read_rdp(&mut core).wrap_err("FLASH_OPTR_RDP read failed")?;
    ensure!(rdp == target_rdp, "failed to persist RDP");

    Ok(())
}
