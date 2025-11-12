use zbus::fdo;

pub trait IntoZResult<T> {
    fn into_z(self) -> fdo::Result<T>;
}

impl<T> IntoZResult<T> for color_eyre::Result<T> {
    #[inline]
    fn into_z(self) -> fdo::Result<T> {
        self.map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}
