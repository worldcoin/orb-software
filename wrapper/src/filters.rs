//! Types related to the filters on a [`crate::camera::Camera`].

use crate::sys;

/// The state of a [`Filter`].
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum FilterState {
    Enabled,
    Disabled,
}

impl From<sys::filter_state_t> for FilterState {
    fn from(value: sys::filter_state_t) -> Self {
        match value {
            sys::filter_state_t::Enabled => Self::Enabled,
            sys::filter_state_t::Disabled => Self::Disabled,
            other => panic!("Unexpected/unknown filter_state_t enum value: {:?}", other),
        }
    }
}

impl From<FilterState> for sys::filter_state_t {
    fn from(value: FilterState) -> Self {
        match value {
            FilterState::Enabled => Self::Enabled,
            FilterState::Disabled => Self::Disabled,
        }
    }
}

/// Enumerates the controllable image processing filters on a [`crate::camera::Camera`].
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum Filter {
    GradientCorrection,
    FlatSceneCorrection,
}

impl From<sys::filter_t> for Filter {
    fn from(value: sys::filter_t) -> Self {
        match value {
            sys::filter_t::GradientCorrection => Self::GradientCorrection,
            sys::filter_t::FlatSceneCorrection => Self::FlatSceneCorrection,
            other => panic!("Unexpected/unknown filter_t enum value: {:?}", other),
        }
    }
}

impl From<Filter> for sys::filter_t {
    fn from(value: Filter) -> Self {
        match value {
            Filter::GradientCorrection => Self::GradientCorrection,
            Filter::FlatSceneCorrection => Self::FlatSceneCorrection,
        }
    }
}

/// The ID of a saved [`Filter::FlatSceneCorrection`].
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum FlatSceneCorrectionId {
    /// The default ID. If a FSC with this ID is stored, then it will be autoloaded
    /// and applied on startup.
    _0,
}

impl From<sys::flat_scene_correction_id_t> for FlatSceneCorrectionId {
    fn from(value: sys::flat_scene_correction_id_t) -> Self {
        match value {
            sys::Id0 => Self::_0,
            other => {
                panic!("Unexpected/unknown flat_scene_correction_id_t enum value: {:?}", other)
            }
        }
    }
}

impl From<FlatSceneCorrectionId> for sys::flat_scene_correction_id_t {
    fn from(value: FlatSceneCorrectionId) -> Self {
        match value {
            FlatSceneCorrectionId::_0 => sys::Id0,
        }
    }
}
