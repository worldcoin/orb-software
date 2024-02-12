#[derive(Debug, Clone, Copy)]
pub struct LinkLayerOptions<const N: usize> {
    pub flags: u8,
}

impl<const N: usize> Default for LinkLayerOptions<N> {
    fn default() -> Self {
        Self { flags: 0 }
    }
}

pub(crate) mod imp {
    use super::LinkLayerOptions;
    use crate::MTU;

    // 	__u8  mtu;	    /* generated & accepted CAN frame type	*/
    // 				    /* __u8 value :				*/
    // 				    /* CAN_MTU   (16) -> standard CAN 2.0	*/
    // 				    /* CANFD_MTU (72) -> CAN FD frame	*/
    //
    // 	__u8  tx_dl;	/* tx link layer data length in bytes	*/
    // 				    /* (configured maximum payload length)	*/
    // 				    /* __u8 value : 8,12,16,20,24,32,48,64	*/
    // 				    /* => rx path supports all LL_DL values */
    //
    // 	__u8  tx_flags;	/* set into struct canfd_frame.flags	*/
    // 				    /* at frame creation: e.g. CANFD_BRS	*/
    // 				    /* Obsolete when the BRS flag is fixed	*/
    // 				    /* by the CAN netdriver configuration	*/
    #[derive(Debug, Clone, Copy)]
    #[repr(C)]
    pub(crate) struct RawLinkLayerOptions {
        mtu: u8,
        tx_dl: u8,
        tx_flags: u8,
    }

    impl Default for RawLinkLayerOptions {
        fn default() -> Self {
            Self {
                mtu: MTU::CAN as u8,
                tx_dl: MTU::to_dlen(MTU::CAN) as u8,
                tx_flags: 0,
            }
        }
    }

    impl<const N: usize> From<LinkLayerOptions<N>> for RawLinkLayerOptions {
        fn from(options: LinkLayerOptions<N>) -> Self {
            // Safe to unwrap as the LinkLayerOptions<N> has a constrained N of 8 or 64
            let dlen: u8 = N.try_into().unwrap();
            let mtu = MTU::from_dlen(N).unwrap().into();
            Self {
                mtu,
                tx_dl: dlen,
                tx_flags: options.flags,
            }
        }
    }
}
