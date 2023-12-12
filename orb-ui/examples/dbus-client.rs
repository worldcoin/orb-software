use zbus::{dbus_proxy, Connection};

#[dbus_proxy(
    default_service = "org.worldcoin.OrbSignupState1",
    default_path = "/org/worldcoin/OrbSignupState1",
    interface = "org.worldcoin.OrbSignupState1"
)]
trait SignupState {
    fn orb_signup_state_event(&self, serialized_event: String) -> zbus::Result<String>;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::session().await?;
    let proxy = SignupStateProxy::new(&connection).await?;

    proxy
        .orb_signup_state_event("\"Bootup\"".to_string())
        .await?;

    Ok(())
}
