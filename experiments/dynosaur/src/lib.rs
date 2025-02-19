use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

/// dyn-compatible behavior
//#[dynosaur::dynosaur(Dyno = dyn Compatible)]
pub trait Compatible {
    fn incompat(&self) -> &impl Incompatible
    where
        Self: Sized;
}

/// dyn-incompatible behavior
pub trait Incompatible: Clone {}

/// A concrete type that implements both traits.
#[derive(Clone)]
pub struct Concrete;

impl Compatible for Concrete {
    fn incompat(&self) -> &impl Incompatible {
        self
    }
}

impl<T: Clone> Incompatible for T {}

fn box_dyn() {
    let boxed: Box<dyn Compatible> = Box::new(Concrete);
    let incompat = boxed.incompat();
}
