//! # D-Bus interface proxy for: `org.freedesktop.login1.Manager`
//!
//! This code was generated by `zbus-xmlgen` `4.1.0` from D-Bus introspection data.
//! Source: `login1.xml`.
//!
//! You may prefer to adapt it, instead of using it verbatim.
//!
//! More information can be found in the [Writing a client proxy] section of the zbus
//! documentation.
//!
//! This type implements the [D-Bus standard interfaces], (`org.freedesktop.DBus.*`) for which the
//! following zbus API can be used:
//!
//! * [`zbus::fdo::PeerProxy`]
//! * [`zbus::fdo::IntrospectableProxy`]
//! * [`zbus::fdo::PropertiesProxy`]
//!
//! Consequently `zbus-xmlgen` did not generate code for the above interfaces.
//!
//! [Writing a client proxy]: https://dbus2.github.io/zbus/client.html
//! [D-Bus standard interfaces]: https://dbus.freedesktop.org/doc/dbus-specification.html#standard-interfaces,
use zbus::proxy;
#[proxy(interface = "org.freedesktop.login1.Manager", assume_defaults = true)]
trait Manager {
    /// ActivateSession method
    fn activate_session(&self, arg_1: &str) -> zbus::Result<()>;

    /// ActivateSessionOnSeat method
    fn activate_session_on_seat(&self, arg_1: &str, arg_2: &str) -> zbus::Result<()>;

    /// AttachDevice method
    fn attach_device(&self, arg_1: &str, arg_2: &str, arg_3: bool) -> zbus::Result<()>;

    /// CanHalt method
    fn can_halt(&self) -> zbus::Result<String>;

    /// CanHibernate method
    fn can_hibernate(&self) -> zbus::Result<String>;

    /// CanHybridSleep method
    fn can_hybrid_sleep(&self) -> zbus::Result<String>;

    /// CanPowerOff method
    fn can_power_off(&self) -> zbus::Result<String>;

    /// CanReboot method
    fn can_reboot(&self) -> zbus::Result<String>;

    /// CanRebootParameter method
    fn can_reboot_parameter(&self) -> zbus::Result<String>;

    /// CanRebootToBootLoaderEntry method
    fn can_reboot_to_boot_loader_entry(&self) -> zbus::Result<String>;

    /// CanRebootToBootLoaderMenu method
    fn can_reboot_to_boot_loader_menu(&self) -> zbus::Result<String>;

    /// CanRebootToFirmwareSetup method
    fn can_reboot_to_firmware_setup(&self) -> zbus::Result<String>;

    /// CanSuspend method
    fn can_suspend(&self) -> zbus::Result<String>;

    /// CanSuspendThenHibernate method
    fn can_suspend_then_hibernate(&self) -> zbus::Result<String>;

    /// CancelScheduledShutdown method
    fn cancel_scheduled_shutdown(&self) -> zbus::Result<bool>;

    /// CreateSession method
    #[allow(clippy::too_many_arguments)]
    fn create_session(
        &self,
        arg_1: u32,
        arg_2: u32,
        arg_3: &str,
        arg_4: &str,
        arg_5: &str,
        arg_6: &str,
        arg_7: &str,
        arg_8: u32,
        arg_9: &str,
        arg_10: &str,
        arg_11: bool,
        arg_12: &str,
        arg_13: &str,
        arg_14: &[&(&str, &zbus::zvariant::Value<'_>)],
    ) -> zbus::Result<(
        String,
        zbus::zvariant::OwnedObjectPath,
        String,
        zbus::zvariant::OwnedFd,
        u32,
        String,
        u32,
        bool,
    )>;

    /// FlushDevices method
    fn flush_devices(&self, arg_1: bool) -> zbus::Result<()>;

    /// GetSeat method
    fn get_seat(&self, arg_1: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// GetSession method
    fn get_session(&self, arg_1: &str)
        -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// GetSessionByPID method
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(
        &self,
        arg_1: u32,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// GetUser method
    fn get_user(&self, arg_1: u32) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// GetUserByPID method
    #[zbus(name = "GetUserByPID")]
    fn get_user_by_pid(
        &self,
        arg_1: u32,
    ) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Halt method
    fn halt(&self, arg_1: bool) -> zbus::Result<()>;

    /// Hibernate method
    fn hibernate(&self, arg_1: bool) -> zbus::Result<()>;

    /// HybridSleep method
    fn hybrid_sleep(&self, arg_1: bool) -> zbus::Result<()>;

    /// Inhibit method
    fn inhibit(
        &self,
        arg_1: &str,
        arg_2: &str,
        arg_3: &str,
        arg_4: &str,
    ) -> zbus::Result<zbus::zvariant::OwnedFd>;

    /// KillSession method
    fn kill_session(&self, arg_1: &str, arg_2: &str, arg_3: i32) -> zbus::Result<()>;

    /// KillUser method
    fn kill_user(&self, arg_1: u32, arg_2: i32) -> zbus::Result<()>;

    /// ListInhibitors method
    fn list_inhibitors(
        &self,
    ) -> zbus::Result<Vec<(String, String, String, String, u32, u32)>>;

    /// ListSeats method
    fn list_seats(
        &self,
    ) -> zbus::Result<Vec<(String, zbus::zvariant::OwnedObjectPath)>>;

    /// ListSessions method
    fn list_sessions(
        &self,
    ) -> zbus::Result<Vec<(String, u32, String, String, zbus::zvariant::OwnedObjectPath)>>;

    /// ListUsers method
    fn list_users(
        &self,
    ) -> zbus::Result<Vec<(u32, String, zbus::zvariant::OwnedObjectPath)>>;

    /// LockSession method
    fn lock_session(&self, arg_1: &str) -> zbus::Result<()>;

    /// LockSessions method
    fn lock_sessions(&self) -> zbus::Result<()>;

    /// PowerOff method
    fn power_off(&self, arg_1: bool) -> zbus::Result<()>;

    /// Reboot method
    fn reboot(&self, arg_1: bool) -> zbus::Result<()>;

    /// ReleaseSession method
    fn release_session(&self, arg_1: &str) -> zbus::Result<()>;

    /// ScheduleShutdown method
    fn schedule_shutdown(&self, arg_1: &str, arg_2: u64) -> zbus::Result<()>;

    /// SetRebootParameter method
    fn set_reboot_parameter(&self, arg_1: &str) -> zbus::Result<()>;

    /// SetRebootToBootLoaderEntry method
    fn set_reboot_to_boot_loader_entry(&self, arg_1: &str) -> zbus::Result<()>;

    /// SetRebootToBootLoaderMenu method
    fn set_reboot_to_boot_loader_menu(&self, arg_1: u64) -> zbus::Result<()>;

    /// SetRebootToFirmwareSetup method
    fn set_reboot_to_firmware_setup(&self, arg_1: bool) -> zbus::Result<()>;

    /// SetUserLinger method
    fn set_user_linger(&self, arg_1: u32, arg_2: bool, arg_3: bool)
        -> zbus::Result<()>;

    /// SetWallMessage method
    fn set_wall_message(&self, arg_1: &str, arg_2: bool) -> zbus::Result<()>;

    /// Suspend method
    fn suspend(&self, arg_1: bool) -> zbus::Result<()>;

    /// SuspendThenHibernate method
    fn suspend_then_hibernate(&self, arg_1: bool) -> zbus::Result<()>;

    /// TerminateSeat method
    fn terminate_seat(&self, arg_1: &str) -> zbus::Result<()>;

    /// TerminateSession method
    fn terminate_session(&self, arg_1: &str) -> zbus::Result<()>;

    /// TerminateUser method
    fn terminate_user(&self, arg_1: u32) -> zbus::Result<()>;

    /// UnlockSession method
    fn unlock_session(&self, arg_1: &str) -> zbus::Result<()>;

    /// UnlockSessions method
    fn unlock_sessions(&self) -> zbus::Result<()>;

    /// PrepareForShutdown signal
    #[zbus(signal)]
    fn prepare_for_shutdown(&self, arg_1: bool) -> zbus::Result<()>;

    /// PrepareForSleep signal
    #[zbus(signal)]
    fn prepare_for_sleep(&self, arg_1: bool) -> zbus::Result<()>;

    /// SeatNew signal
    #[zbus(signal)]
    fn seat_new(
        &self,
        arg_1: &str,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// SeatRemoved signal
    #[zbus(signal)]
    fn seat_removed(
        &self,
        arg_1: &str,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// SessionNew signal
    #[zbus(signal)]
    fn session_new(
        &self,
        arg_1: &str,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// SessionRemoved signal
    #[zbus(signal)]
    fn session_removed(
        &self,
        arg_1: &str,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// UserNew signal
    #[zbus(signal)]
    fn user_new(
        &self,
        arg_1: u32,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// UserRemoved signal
    #[zbus(signal)]
    fn user_removed(
        &self,
        arg_1: u32,
        arg_2: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::Result<()>;

    /// BlockInhibited property
    #[zbus(property)]
    fn block_inhibited(&self) -> zbus::Result<String>;

    /// BootLoaderEntries property
    #[zbus(property)]
    fn boot_loader_entries(&self) -> zbus::Result<Vec<String>>;

    /// DelayInhibited property
    #[zbus(property)]
    fn delay_inhibited(&self) -> zbus::Result<String>;

    /// Docked property
    #[zbus(property)]
    fn docked(&self) -> zbus::Result<bool>;

    /// EnableWallMessages property
    #[zbus(property)]
    fn enable_wall_messages(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_enable_wall_messages(&self, value: bool) -> zbus::Result<()>;

    /// HandleHibernateKey property
    #[zbus(property)]
    fn handle_hibernate_key(&self) -> zbus::Result<String>;

    /// HandleLidSwitch property
    #[zbus(property)]
    fn handle_lid_switch(&self) -> zbus::Result<String>;

    /// HandleLidSwitchDocked property
    #[zbus(property)]
    fn handle_lid_switch_docked(&self) -> zbus::Result<String>;

    /// HandleLidSwitchExternalPower property
    #[zbus(property)]
    fn handle_lid_switch_external_power(&self) -> zbus::Result<String>;

    /// HandlePowerKey property
    #[zbus(property)]
    fn handle_power_key(&self) -> zbus::Result<String>;

    /// HandleSuspendKey property
    #[zbus(property)]
    fn handle_suspend_key(&self) -> zbus::Result<String>;

    /// HoldoffTimeoutUSec property
    #[zbus(property, name = "HoldoffTimeoutUSec")]
    fn holdoff_timeout_usec(&self) -> zbus::Result<u64>;

    /// IdleAction property
    #[zbus(property)]
    fn idle_action(&self) -> zbus::Result<String>;

    /// IdleActionUSec property
    #[zbus(property, name = "IdleActionUSec")]
    fn idle_action_usec(&self) -> zbus::Result<u64>;

    /// IdleHint property
    #[zbus(property)]
    fn idle_hint(&self) -> zbus::Result<bool>;

    /// IdleSinceHint property
    #[zbus(property)]
    fn idle_since_hint(&self) -> zbus::Result<u64>;

    /// IdleSinceHintMonotonic property
    #[zbus(property)]
    fn idle_since_hint_monotonic(&self) -> zbus::Result<u64>;

    /// InhibitDelayMaxUSec property
    #[zbus(property, name = "InhibitDelayMaxUSec")]
    fn inhibit_delay_max_usec(&self) -> zbus::Result<u64>;

    /// InhibitorsMax property
    #[zbus(property)]
    fn inhibitors_max(&self) -> zbus::Result<u64>;

    /// KillExcludeUsers property
    #[zbus(property)]
    fn kill_exclude_users(&self) -> zbus::Result<Vec<String>>;

    /// KillOnlyUsers property
    #[zbus(property)]
    fn kill_only_users(&self) -> zbus::Result<Vec<String>>;

    /// KillUserProcesses property
    #[zbus(property)]
    fn kill_user_processes(&self) -> zbus::Result<bool>;

    /// LidClosed property
    #[zbus(property)]
    fn lid_closed(&self) -> zbus::Result<bool>;

    /// NAutoVTs property
    #[zbus(property, name = "NAutoVTs")]
    fn nauto_vts(&self) -> zbus::Result<u32>;

    /// NCurrentInhibitors property
    #[zbus(property, name = "NCurrentInhibitors")]
    fn ncurrent_inhibitors(&self) -> zbus::Result<u64>;

    /// NCurrentSessions property
    #[zbus(property, name = "NCurrentSessions")]
    fn ncurrent_sessions(&self) -> zbus::Result<u64>;

    /// OnExternalPower property
    #[zbus(property)]
    fn on_external_power(&self) -> zbus::Result<bool>;

    /// PreparingForShutdown property
    #[zbus(property)]
    fn preparing_for_shutdown(&self) -> zbus::Result<bool>;

    /// PreparingForSleep property
    #[zbus(property)]
    fn preparing_for_sleep(&self) -> zbus::Result<bool>;

    /// RebootParameter property
    #[zbus(property)]
    fn reboot_parameter(&self) -> zbus::Result<String>;

    /// RebootToBootLoaderEntry property
    #[zbus(property)]
    fn reboot_to_boot_loader_entry(&self) -> zbus::Result<String>;

    /// RebootToBootLoaderMenu property
    #[zbus(property)]
    fn reboot_to_boot_loader_menu(&self) -> zbus::Result<u64>;

    /// RebootToFirmwareSetup property
    #[zbus(property)]
    fn reboot_to_firmware_setup(&self) -> zbus::Result<bool>;

    /// RemoveIPC property
    #[zbus(property, name = "RemoveIPC")]
    fn remove_ipc(&self) -> zbus::Result<bool>;

    /// RuntimeDirectorySize property
    #[zbus(property)]
    fn runtime_directory_size(&self) -> zbus::Result<u64>;

    /// ScheduledShutdown property
    #[zbus(property)]
    fn scheduled_shutdown(&self) -> zbus::Result<(String, u64)>;

    /// SessionsMax property
    #[zbus(property)]
    fn sessions_max(&self) -> zbus::Result<u64>;

    /// UserStopDelayUSec property
    #[zbus(property, name = "UserStopDelayUSec")]
    fn user_stop_delay_usec(&self) -> zbus::Result<u64>;

    /// MANUALLY TWEAKED (1)
    /// WallMessage property
    #[zbus(name = "WallMessage", property)]
    fn wall_message_property(&self) -> zbus::Result<String>;

    /// MANUALLY TWEAKED (1)
    /// WallMessage property
    #[zbus(name = "WallMessage", property)]
    fn set_wall_message_property(&self, value: &str) -> zbus::Result<()>;
}