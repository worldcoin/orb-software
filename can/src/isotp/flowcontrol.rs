#[derive(Debug, Clone, Copy)]
pub struct FlowControlOptions {
    pub block_size: Blocksize,
    pub seperation_time: SeparationTime,
    pub wait_transmission: WaitFrameTransmission,
}

impl Default for FlowControlOptions {
    fn default() -> Self {
        Self {
            block_size: Blocksize::Off,
            seperation_time: SeparationTime::Off,
            wait_transmission: WaitFrameTransmission::Off,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Blocksize {
    Off,
    Limited(u8),
}

#[derive(Debug, Clone, Copy)]
pub enum WaitFrameTransmission {
    Off,
    Frames(u8),
}

#[derive(Debug, Clone, Copy)]
pub enum SeparationTime {
    Off,
    Coarse(u8),
    Fine(u8),
}

pub const CAN_ISOTP_FC_ST_COARSE_MASK: u8 = 0x7F;
pub const CAN_ISOTP_FC_ST_FINE_MASK: u8 = 0xF9;

pub(crate) mod imp {
    use super::{
        Blocksize, FlowControlOptions, SeparationTime, WaitFrameTransmission,
        CAN_ISOTP_FC_ST_COARSE_MASK, CAN_ISOTP_FC_ST_FINE_MASK,
    };

    // (From 'include/socketcan/can/isotp.h')
    // Remark on CAN_ISOTP_DEFAULT_RECV_* values:
    //
    // We can strongly assume, that the Linux Kernel implementation of
    // CAN_ISOTP is capable to run with BS=0, STmin=0 and WFTmax=0.
    // But as we like to be able to behave as a commonly available ECU,
    // these default settings can be changed via sockopts.
    // For that reason the STmin value is intentionally _not_ checked for
    // consistency and copied directly into the flow control (FC) frame.
    //
    // -----
    //
    // __u8  bs;	/* blocksize provided in FC frame	*/
    // 			    /* __u8 value : blocksize. 0 = off	*/
    //
    // __u8  stmin; /* separation time provided in FC frame	*/
    // 			    /* __u8 value :				*/
    // 			    /* 0x00 - 0x7F : 0 - 127 ms		*/
    // 			    /* 0x80 - 0xF0 : reserved		*/
    // 			    /* 0xF1 - 0xF9 : 100 us - 900 us	*/
    // 			    /* 0xFA - 0xFF : reserved		*/
    //
    // __u8  wftmax;    /* max. number of wait frame transmiss.	*/
    //                  /* __u8 value : 0 = omit FC N_PDU WT	*/
    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    #[derive(Default)]
    pub(crate) struct RawFlowControlOptions {
        bs: u8,
        stmin: u8,
        wftmax: u8,
    }

    impl From<FlowControlOptions> for RawFlowControlOptions {
        fn from(fco: FlowControlOptions) -> Self {
            Self {
                bs: match fco.block_size {
                    Blocksize::Off => 0,
                    Blocksize::Limited(lim) => lim,
                },
                stmin: match fco.seperation_time {
                    SeparationTime::Off => 0,
                    SeparationTime::Coarse(val) => val & CAN_ISOTP_FC_ST_COARSE_MASK,
                    SeparationTime::Fine(val) => val & CAN_ISOTP_FC_ST_FINE_MASK,
                },
                wftmax: match fco.wait_transmission {
                    WaitFrameTransmission::Off => 0,
                    WaitFrameTransmission::Frames(frames) => frames,
                },
            }
        }
    }
}
