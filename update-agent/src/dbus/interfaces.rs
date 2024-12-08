use zbus::{
    blocking::{connection, object_server::InterfaceRef, Connection},
    interface,
};

const OBJECT_PATH: &str = "/org/worldcoin/OrbUpdateAgent/Status";
const BUS_NAME: &str = "org.worldcoin.OrbUpdateAgent";

pub struct UpdateStatus {
    status: String,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            status: "none".to_string(),
        }
    }
}

#[interface(name = "org.worldcoin.OrbUpdateAgent1.Status")]
impl UpdateStatus {
    #[zbus(property)]
    fn update_status(&self) -> &str {
        &self.status
    }

    #[zbus(property)]
    fn set_update_status(&mut self, value: String) {
        self.status = value;
    }
}

impl UpdateStatus {
    /// Create a new connection to D-Bus and serve the `UpdateStatus` interface.
    pub fn create_dbus_conn() -> zbus::Result<Connection> {
        connection::Builder::session()?
            .name(BUS_NAME)?
            .serve_at(OBJECT_PATH, UpdateStatus::default())?
            .build()
    }

    /// Get a reference to the underlying interface of `UpdateStatus`
    fn get_iface_ref(
        conn: &Connection,
    ) -> Result<InterfaceRef<UpdateStatus>, zbus::Error> {
        let object_server = conn.object_server();
        object_server.interface::<_, UpdateStatus>(OBJECT_PATH)
    }

    /// Set the UpdateStatus property to the given value
    pub fn set_conn_status(
        conn: &Connection,
        status: String,
    ) -> Result<(), zbus::Error> {
        let iface = UpdateStatus::get_iface_ref(conn)?;
        iface.get_mut().set_update_status(status);
        Ok(())
    }
}
