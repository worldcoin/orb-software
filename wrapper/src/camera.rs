use core::{ffi::c_void, mem::MaybeUninit, ptr};
use std::{ffi::CStr, mem, panic::catch_unwind, ptr::NonNull};

use log::error;

use crate::{
    error::{ErrorCode, Result},
    frame::FrameContainer,
    frame_format::FrameFormat,
    sys::{self, frame_t, seekcamera_t},
};

// Note: This type is a fat pointer.
type BoxDynCallback = Box<dyn FnMut(FrameContainer) + Send + 'static>;

#[derive(Debug)]
pub struct Camera {
    ptr: *mut seekcamera_t,
    closure_ptr: Option<NonNull<BoxDynCallback>>,
    pairing_status: PairingStatus,
}

impl Camera {
    pub(crate) unsafe fn new(ptr: *mut seekcamera_t, pairing_status: PairingStatus) -> Self {
        Self { ptr, closure_ptr: None, pairing_status }
    }

    /// Runs `cb` whenever a new frame is received.
    ///
    /// Note: `cb` will execute on a separate thread managed by the thermal camera SDK.
    /// For this reason, it must be `Send`.
    pub fn set_callback(&mut self, cb: BoxDynCallback) -> Result<()> {
        let closure_ptr = unsafe { register_callback(self.ptr, cb) }?;
        // Replace and drop the old closure
        if let Some(old_closure_ptr) = self.closure_ptr.replace(closure_ptr) {
            unsafe { drop_closure(old_closure_ptr) };
        }

        Ok(())
    }

    pub fn clear_callback(&mut self) -> Result<()> {
        extern "C" fn noop_fn(
            _cam_ptr: *mut seekcamera_t,
            _frame_ptr: *mut frame_t,
            _data: *mut c_void,
        ) {
        }
        let err = unsafe {
            sys::register_frame_available_callback(self.ptr, Some(noop_fn), ptr::null_mut())
        };
        ErrorCode::result_from_sys(err)?;

        // Remove the old closure, and then drop it.
        if let Some(old_closure_ptr) = self.closure_ptr.take() {
            unsafe { drop_closure(old_closure_ptr) };
        }

        Ok(())
    }

    pub fn serial_number(&mut self) -> Result<SerialNumber> {
        let mut serial: MaybeUninit<sys::serial_number_t> = MaybeUninit::uninit();
        let err = unsafe { sys::get_serial_number(self.ptr, serial.as_mut_ptr()) };
        ErrorCode::result_from_sys(err).map(|()| SerialNumber(unsafe { serial.assume_init() }))
    }

    pub fn chip_id(&mut self) -> Result<ChipId> {
        let mut cid: MaybeUninit<sys::chipid_t> = MaybeUninit::uninit();
        let err = unsafe { sys::get_chipid(self.ptr, cid.as_mut_ptr()) };
        ErrorCode::result_from_sys(err).map(|()| ChipId(unsafe { cid.assume_init() }))
    }

    pub fn ptr_mut(&mut self) -> *mut seekcamera_t {
        self.ptr
    }

    pub fn capture_session_start(&mut self, frame_fmt: FrameFormat) -> Result<()> {
        let err = unsafe { sys::capture_session_start(self.ptr, frame_fmt as _) };
        ErrorCode::result_from_sys(err)
    }

    pub fn capture_session_stop(&mut self) -> Result<()> {
        let err = unsafe { sys::capture_session_stop(self.ptr) };
        ErrorCode::result_from_sys(err)
    }

    /// Stores calibration data, paring the sensor.
    ///
    /// # Args
    /// - `source_dir`: If `None`, will use calibration data from sensor flash.
    ///   Otherwise, expects a path to a directory containing containing any
    ///   number of subdirectories whose names correspond exactly to the unique
    ///   camera chip identifier (CID).
    ///
    ///   For example:
    ///   ```ignore
    ///   source-dir/
    ///     [CID1]/
    ///         ...
    ///     [CID2]/
    ///         ...
    ///     [CID3]/
    ///         ...
    ///     ...
    ///     [CIDN]
    ///         ...
    ///     ```
    /// - `progress_cb`: If not `None`, will call this function with the progress
    /// percentage as a value from \[0,100\]
    pub fn store_calibration_data(
        &mut self,
        source_dir: Option<&CStr>,
        progress_cb: Option<fn(u8)>,
    ) -> Result<()> {
        unsafe extern "C" fn ffi_fn(pct: usize, data: *mut c_void) {
            let fn_ptr: fn(u8) = unsafe { core::mem::transmute(data) };
            if let Err(err) = catch_unwind(|| fn_ptr(pct as u8)) {
                log::error!("Error in progress callback: {err:?}");
            }
        }

        let (fn_ptr, data): (sys::memory_access_callback_t, _) =
            if let Some(progress_cb) = progress_cb {
                (Some(ffi_fn), progress_cb as *mut c_void)
            } else {
                (None, ptr::null_mut())
            };

        let err = unsafe {
            sys::store_calibration_data(
                self.ptr,
                source_dir.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
                fn_ptr,
                data,
            )
        };
        ErrorCode::result_from_sys(err)?;
        self.pairing_status = PairingStatus::Paired;
        Ok(())
    }

    pub fn is_paired(&self) -> bool {
        self.pairing_status == PairingStatus::Paired
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        let result = self.clear_callback().and_then(|_| self.capture_session_stop());
        if let Err(err) = result {
            error!("Failed to clear camera callback: {err}");
        }
    }
}

unsafe impl Send for Camera {}
unsafe impl Sync for Camera {}

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
#[repr(transparent)]
pub struct SerialNumber(sys::serial_number_t);

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
#[repr(transparent)]
pub struct ChipId(sys::chipid_t);

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PairingStatus {
    Paired,
    Unpaired,
}

//-----------------------//
// ---- Helper code ---- //
//-----------------------//

unsafe fn drop_closure(closure_ptr: NonNull<BoxDynCallback>) {
    debug_assert_eq!(
        mem::size_of::<*mut c_void>(),
        mem::size_of_val(&closure_ptr),
        "should have been impossible to get a fat pointer",
    );
    drop(unsafe { Box::from_raw(closure_ptr.as_ptr()) });
}

/// Safety: `cam_ptr` must be valid.
unsafe fn register_callback(
    cam_ptr: *mut sys::seekcamera_t,
    fat_closure_box: BoxDynCallback,
) -> Result<NonNull<BoxDynCallback>> {
    /// The `extern "C"` function that the seek SDK will invoke in the callback.
    unsafe extern "C" fn callback_wrapper(
        _cam_ptr: *mut seekcamera_t,
        frame_ptr: *mut frame_t,
        data: *mut c_void,
    ) {
        // SAFETY: Panics in c are UB, so we catch it and log it instead of bubbling up.
        let result = catch_unwind(|| {
            // Safety: Because BoxDynCallback is sized, &BoxDynCallback will not be a fat
            // pointer. Also, it is 'static so it cannot hold any references that
            // might be dangling.
            let closure: &mut BoxDynCallback =
                unsafe { data.cast::<BoxDynCallback>().as_mut() }.unwrap();
            // As best as I can tell, the lifetime is inferred to be 'static here. That would
            // be a problem except for the fact that `closure` has a HRTB for the lifetime of the
            // container, meaning that the closure cannot assume any specific lifetime and
            // therefore will be pessimistic about how long the lifetime it was given will live for.
            // This prevents the closure from accidentally hanging onto the lifetime longer than
            // the lexical scope of the closure argument.
            let frame_container = unsafe { FrameContainer::new(frame_ptr) };
            closure(frame_container);
        });

        if let Err(err) = result {
            log::error!("Unexpected panic in camera callback: \n{:?}", err);
        }
    }

    // We box the closure, to make it castable to *mut c_void.
    // Otherwise, casting will strip the vtable.
    let skinny_closure_box = Box::new(fat_closure_box);
    debug_assert_eq!(
        mem::size_of::<*mut c_void>(),
        mem::size_of_val(&skinny_closure_box),
        "should have been impossible to get a fat pointer"
    );
    let closure_ptr = NonNull::new(Box::into_raw(skinny_closure_box)).unwrap();

    let err = unsafe {
        sys::register_frame_available_callback(
            cam_ptr,
            Some(callback_wrapper),
            closure_ptr.as_ptr().cast::<c_void>(),
        )
    };

    let result = ErrorCode::result_from_sys(err);
    match result {
        Ok(()) => Ok(closure_ptr),
        Err(err) => {
            // Drop the closure, to avoid leaking memory.
            unsafe { drop_closure(closure_ptr) };
            Err(err)
        }
    }
}
