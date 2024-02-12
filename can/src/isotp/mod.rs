pub mod addr;
pub mod flowcontrol;
pub mod linklayer;
pub mod socket_isotp;
pub mod stream;

/// Defined in the Kernel as SOL_CAN_BASE + CAN_ISOTP, which comes out to 106
pub const SOL_CAN_ISOTP: libc::c_int = 106;
pub const CAN_ISOTP_OPTS: libc::c_int = 1;
pub const CAN_ISOTP_RECV_FC: libc::c_int = 2;
pub const CAN_ISOTP_TX_STMIN: libc::c_int = 3;
pub const CAN_ISOTP_RX_STMIN: libc::c_int = 4;
pub const CAN_ISOTP_LL_OPTS: libc::c_int = 5;

#[derive(Debug, Clone, Copy)]
pub struct IsotpOptions {
    flags: u32,
    transmission_time_nano: u32,
    extended_address: u8,
    tx_padding_content: u8,
    rx_padding_content: u8,
    rx_extended_address: u8,
}

impl Default for IsotpOptions {
    fn default() -> Self {
        Self {
            flags: 0,
            transmission_time_nano: 0,
            extended_address: 0,
            tx_padding_content: 0xCC,
            rx_padding_content: 0xCC,
            rx_extended_address: 0,
        }
    }
}

#[repr(u32)]
pub enum IsotpFlags {
    /// Disables sending of Flow Control frames
    ListenMode = 0x001,
    /// Enable extended addressing
    ExtendAddr = 0x002,
    /// Enable CAN frame padding TX path
    TxPadding = 0x004,
    /// Enable CAN frame padding RX path
    RxPadding = 0x008,
    /// Check received CAN frame padding
    CheckPadLength = 0x010,
    /// Check received CAN frame padding
    CheckPadData = 0x020,
    /// Half duplex error state handling
    HalfDuplex = 0x040,
    /// Ignore separation time min from received FC (Flow Control)
    ForceTxSeparationTimeMin = 0x080,
    /// Ignore CFs (Control Frames) depending on RX seperation time min
    ForceRxSeparationTimeMin = 0x100,
    /// Different RX extended addressing
    RxExtendAddr = 0x200,
}

pub(crate) mod imp {
    use super::IsotpOptions;

    // 	__u32 flags;	    	/* set flags for isotp behaviour.	*/
    // 		            		/* __u32 value : flags see below	*/
    //
    // 	__u32 frame_txtime;	    /* frame transmission time (N_As/N_Ar)	*/
    //          				/* __u32 value : time in nano secs	*/
    //
    // 	__u8  ext_address;	    /* set address for extended addressing	*/
    // 		            		/* __u8 value : extended address	*/
    //
    // 	__u8  txpad_content;	/* set content of padding byte (tx)	*/
    //          				/* __u8 value : content	on tx path	*/
    //
    // 	__u8  rxpad_content;	/* set content of padding byte (rx)	*/
    // 		            		/* __u8 value : content	on rx path	*/
    //
    // 	__u8  rx_ext_address;	/* set address for extended addressing	*/
    //          				/* __u8 value : extended address (rx)	*/
    #[derive(Debug, Copy, Clone)]
    #[repr(C)]
    pub(crate) struct RawIsotpOptions {
        flags: u32,
        frame_txtime: u32,
        ext_address: u8,
        txpad_content: u8,
        rxpad_content: u8,
        rx_ext_address: u8,
    }

    impl Default for RawIsotpOptions {
        fn default() -> Self {
            Self {
                flags: 0,
                frame_txtime: 0,
                ext_address: 0,
                txpad_content: 0xCC,
                rxpad_content: 0xCC,
                rx_ext_address: 0x00,
            }
        }
    }

    impl From<IsotpOptions> for RawIsotpOptions {
        fn from(options: IsotpOptions) -> Self {
            Self {
                flags: options.flags,
                frame_txtime: options.transmission_time_nano,
                ext_address: options.extended_address,
                txpad_content: options.tx_padding_content,
                rxpad_content: options.rx_padding_content,
                rx_ext_address: options.rx_extended_address,
            }
        }
    }
}
