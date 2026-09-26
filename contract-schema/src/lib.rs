/// Marks a struct or enum as a registered JSON Schema contract.
///
/// Generic types are not supported because they do not identify a concrete
/// schema.
///
/// ```compile_fail
/// use orb_contract_schema::contract;
///
/// #[contract]
/// struct Response<T> {
///     value: T,
/// }
/// ```
pub use orb_contract_schema_macros::contract;
pub use schemars::Schema;

pub struct Contract {
    pub name: &'static str,
    pub schema: fn() -> Schema,
}

inventory::collect!(Contract);

pub fn gen_schemas() -> impl Iterator<Item = &'static Contract> {
    inventory::iter::<Contract>.into_iter()
}

#[doc(hidden)]
pub mod __private {
    pub use inventory;
}
