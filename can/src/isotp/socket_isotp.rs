pub(crate) mod imp {
    use std::{io, os::unix::prelude::AsRawFd};

    use crate::{
        isotp::{
            flowcontrol::imp::RawFlowControlOptions, imp::RawIsotpOptions,
            linklayer::imp::RawLinkLayerOptions, CAN_ISOTP_LL_OPTS, CAN_ISOTP_OPTS,
            CAN_ISOTP_RECV_FC, SOL_CAN_ISOTP,
        },
        Error,
    };

    pub(crate) fn set_isotp_opts<T: AsRawFd, O: Into<RawIsotpOptions> + Copy>(
        fd: T,
        opts: O,
    ) -> Result<(), Error> {
        let ret = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                SOL_CAN_ISOTP,
                CAN_ISOTP_OPTS,
                &opts.into() as *const RawIsotpOptions as *const libc::c_void,
                std::mem::size_of::<RawIsotpOptions>() as u32,
            )
        };
        if ret < 0 {
            return Err(Error::Syscall {
                syscall: "setsockopt(2)".to_string(),
                context: Some(
                    format!("setting CAN_ISOTP_OPTS ({:#?})", &opts.into()).to_string(),
                ),
                source: io::Error::last_os_error(),
            });
        }
        Ok(())
    }

    pub(crate) fn set_flow_control_opts<
        T: AsRawFd,
        O: Into<RawFlowControlOptions> + Copy,
    >(
        fd: T,
        opts: O,
    ) -> Result<(), Error> {
        let ret = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                SOL_CAN_ISOTP,
                CAN_ISOTP_RECV_FC,
                &opts.into() as *const RawFlowControlOptions as *const libc::c_void,
                std::mem::size_of::<RawFlowControlOptions>() as u32,
            )
        };
        if ret < 0 {
            return Err(Error::Syscall {
                syscall: "setsockopt(2)".to_string(),
                context: Some(
                    format!("setting CAN_ISOTP_RECV_FC ({:#?})", &opts.into())
                        .to_string(),
                ),
                source: io::Error::last_os_error(),
            });
        }
        Ok(())
    }

    pub(crate) fn set_link_layer_opts<
        T: AsRawFd,
        O: Into<RawLinkLayerOptions> + Copy,
    >(
        fd: T,
        opts: O,
    ) -> Result<(), Error> {
        let ret = unsafe {
            libc::setsockopt(
                fd.as_raw_fd(),
                SOL_CAN_ISOTP,
                CAN_ISOTP_LL_OPTS,
                &opts.into() as *const RawLinkLayerOptions as *const libc::c_void,
                std::mem::size_of::<RawLinkLayerOptions>() as u32,
            )
        };
        if ret < 0 {
            return Err(Error::Syscall {
                syscall: "setsockopt(2)".to_string(),
                context: Some(
                    format!("setting CAN_ISOTP_LL_OPTS ({:#?})", &opts.into())
                        .to_string(),
                ),
                source: io::Error::last_os_error(),
            });
        }
        Ok(())
    }
}
