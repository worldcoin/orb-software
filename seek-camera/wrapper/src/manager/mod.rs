mod error;
mod event;

pub use self::{error::ManagerError, event::Event};

use self::error::Result;
use crate::{
    camera::{Camera, PairingStatus},
    error::ErrorCode,
    sys, SerialNumber,
};
use sys::{manager_t, seekcamera_t};

use log::{error, info, warn};
use std::{
    collections::HashMap,
    ffi::c_void,
    mem,
    panic::catch_unwind,
    ptr::NonNull,
    sync::{Arc, Mutex, MutexGuard},
};

type BoxDynCallback =
    Box<dyn FnMut(*mut seekcamera_t, Event, Option<ErrorCode>) + Send + 'static>;

// A Vec based solution with stable indices (such as Vec<Option<T>> or SlotMap<T>) is
// faster, but for simplicity and fewer dependencies, I'm using a hash map.
pub(crate) type Cameras = HashMap<CameraHandle, Camera>;

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub struct CameraHandle(SerialNumber);

#[derive(Debug)]
pub struct Manager {
    mngr: *mut sys::manager_t,
    closure_ptr: Option<NonNull<BoxDynCallback>>,
    cameras: Arc<Mutex<Cameras>>,
}
impl Manager {
    pub fn new() -> Result<Self> {
        // Default behavior logs to file, we log to stdout/err instead.
        // Technically unsound but its fine?....
        unsafe {
            std::env::set_var("SEEKTHERMAL_LOG_STDOUT", "1");
            std::env::set_var("SEEKTHERMAL_LOG_STDERR", "1");
        }
        let mut mngr = core::ptr::null_mut();
        // TODO: Allow specifying which modes
        let err = unsafe { sys::manager_create(&mut mngr, sys::io_type_t::Usb.0) };
        ErrorCode::result_from_sys(err)?;

        Ok(Self {
            mngr,
            closure_ptr: None,
            cameras: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// # Errors
    /// Will return [`ErrorCode::NotSupported`] if an attempt to set it twice occurs.
    pub fn set_callback(
        &mut self,
        mut cb: impl FnMut(CameraHandle, Event, Option<ErrorCode>) + Send + 'static,
    ) -> Result<()> {
        if self.closure_ptr.is_some() {
            // The underlying SDK would return this to us anyway, but we check in
            // advance to be safe.
            return Err(ErrorCode::NotSupported.into());
        }

        let cameras = Arc::clone(&self.cameras);

        // We have to use a trait object here, to make the type not generic. This
        // gives us a concrete type to cast back to when we eventually drop the closure.
        // An alternative approach would be to store a function pointer (monomorphized with
        // the `T` of the closure type) which drops the closure, but using `Box<dyn T>` is more
        // intuitive and doesn't require storing this function pointer.
        let fat_closure_box: BoxDynCallback =
            Box::new(move |cam_ptr, event, event_status| {
                handle_event(&cameras, &mut cb, cam_ptr, event, event_status)
            });
        let closure_ptr = unsafe {
            register_callback(self.mngr, fat_closure_box)
                .expect("Failed to register closure")
        };
        self.closure_ptr = Some(closure_ptr);

        Ok(())
    }

    pub fn cameras(&self) -> Result<MutexGuard<Cameras>> {
        self.cameras.lock().map_err(ManagerError::from)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        let err = unsafe { sys::manager_destroy(&mut self.mngr) };
        if let Err(err) = ErrorCode::result_from_sys(err) {
            error!("Unexpectedly errored while destroying the seek camera: {err}",)
        }
    }
}

unsafe impl Send for Manager {}
unsafe impl Sync for Manager {}

//-----------------------//
// ---- Helper code ---- //
//-----------------------//

fn handle_event<F: FnMut(CameraHandle, Event, Option<ErrorCode>)>(
    cameras: &Mutex<Cameras>,
    cb: &mut F,
    cam_ptr: *mut seekcamera_t,
    event: Event,
    event_status: Option<ErrorCode>,
) {
    let mut cameras_lock = cameras.lock().unwrap();
    match event {
        Event::Connect => {
            info!("Camera connected");
            let cam_handle =
                add_camera(&mut cameras_lock, cam_ptr, PairingStatus::Paired)
                    .expect("Failed to add camera");
            drop(cameras_lock); // important to avoid deadlock
            cb(cam_handle, event, event_status)
        }
        Event::Disconnect => {
            info!("Camera disconnected");
            if let Some(cam_handle) = find_camera_handle(cam_ptr, &mut cameras_lock) {
                drop(cameras_lock); // important to avoid deadlock
                cb(cam_handle, event, event_status);
                cameras.lock().unwrap().remove(&cam_handle).unwrap();
            } else {
                warn!("Could not find camera handle while disconnecting!");
            }
        }
        Event::Error => {
            error!("Camera error: {event:?}");
        }
        Event::ReadyToPair => {
            info!("Camera ready to pair");
            let cam_handle =
                add_camera(&mut cameras_lock, cam_ptr, PairingStatus::Unpaired)
                    .expect("Failed to add camera");
            drop(cameras_lock); // important to avoid deadlock
            cb(cam_handle, event, event_status);
        }
    }
}

fn find_camera_handle(
    c_ptr: *mut seekcamera_t,
    hmap: &mut HashMap<CameraHandle, Camera>,
) -> Option<CameraHandle> {
    // TODO: Surely there is a safer way than comparing raw memory addresses...
    for (h, c) in hmap.iter_mut() {
        if c_ptr == c.ptr_mut() {
            return Some(*h);
        }
    }
    None
}

fn add_camera(
    cameras: &mut Cameras,
    cam_ptr: *mut seekcamera_t,
    pairing_status: PairingStatus,
) -> Result<CameraHandle> {
    let mut cam = unsafe { Camera::new(cam_ptr, pairing_status) };
    let serial = cam.serial_number()?;
    let handle = CameraHandle(serial);
    assert!(
        cameras.insert(handle, cam).is_none(),
        "It should not be possible for two cameras to have the same serial number"
    );
    Ok(handle)
}

unsafe fn drop_closure(closure_ptr: NonNull<BoxDynCallback>) {
    debug_assert_eq!(
        mem::size_of::<*mut c_void>(),
        mem::size_of_val(&closure_ptr),
        "should have been impossible to get a fat pointer",
    );
    drop(unsafe { Box::from_raw(closure_ptr.as_ptr()) });
}

/// Registers `fat_closure_box` as the callback that will receive events from the manager. Returns
/// a pointer to the closure that should be passed to `drop_closure()` when the callback is
/// deregistered.
///
/// Safety: `mngr` must be valid.
unsafe fn register_callback(
    mngr: *mut manager_t,
    fat_closure_box: BoxDynCallback,
) -> Result<NonNull<BoxDynCallback>> {
    unsafe extern "C" fn callback_wrapper(
        camera_ptr: *mut seekcamera_t,
        event: sys::manager_event_t,
        event_status: sys::error_t,
        closure: *mut c_void,
    ) {
        // SAFETY: Panics in c are UB, so we catch it and log it instead of bubbling up.
        let result = catch_unwind(|| {
            let event = Event::try_from(event).expect("Unexpected c enum variant");
            let event_status = ErrorCode::result_from_sys(event_status).err();
            // Safety: Because `BoxDynCallback` is sized, &BoxDynCallback will not be a fat
            // pointer. Also, `BoxDynCallback` is 'static so it cannot hold any references that
            // might be dangling.
            let closure: &mut BoxDynCallback =
                unsafe { closure.cast::<BoxDynCallback>().as_mut() }.unwrap();
            closure(camera_ptr, event, event_status);
        });
        if let Err(err) = result {
            log::error!("Unexpected panic in manager callback:\n{:?}", err);
        }
    }

    // We box the closure again, to make it castable to *mut c_void.
    // This is because casting a fat pointer to void* will strip the vtable.
    let skinny_closure_box = Box::new(fat_closure_box);
    debug_assert_eq!(
        mem::size_of::<*mut c_void>(),
        mem::size_of_val(&skinny_closure_box),
        "should have been impossible to get a fat pointer"
    );
    let closure_ptr = NonNull::new(Box::into_raw(skinny_closure_box)).unwrap();

    let err = unsafe {
        sys::manager_register_event_callback(
            mngr,
            Some(callback_wrapper),
            closure_ptr.as_ptr().cast::<c_void>(),
        )
    };
    let result = ErrorCode::result_from_sys(err);

    match result {
        Ok(()) => Ok(closure_ptr),
        Err(err) => {
            // Drop the closure so we don't leak memory.
            unsafe { drop_closure(closure_ptr) };
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_fork::rusty_fork_test;

    rusty_fork_test! {
        #[test]
        fn test_double_set_callback() {
            let dir = tempfile::tempdir().unwrap();
            // TODO: Fix this shit
            unsafe {
                std::env::set_var("SEEKTHERMAL_ROOT", dir.path());
            }
            let mut m = Manager::new().expect("Manager should be created");
            m.set_callback(|_, _, _| {}).expect("First callback should be set");
            assert!(
                matches!(m.set_callback(|_, _, _| {}), Err(ManagerError::SdkError(ErrorCode::NotSupported))),
                "second callback should fail to set"
            );
        }
    }
}
