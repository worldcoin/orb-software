mod fixture;

use fixture::Fixture;
use zbus::{fdo::DBusProxy, names::BusName};

#[tokio::test]
async fn it_exposes_a_service_in_dbus() {
    // Arrange
    let fx = Fixture::new().await;

    let dbus = DBusProxy::new(&fx.dbus).await.unwrap();
    let name =
        BusName::try_from(orb_backend_status_dbus::constants::SERVICE_NAME).unwrap();

    // Act
    fx.log().start().await;
    let has_owner = dbus.name_has_owner(name).await.unwrap();

    // Assert
    assert!(has_owner)
}
