//! OFD document model.
//! Mirrors GB/T 33190-2016 as pure data with no I/O, so parsing, writing and
//! tests share these types. Element names follow the standard exactly.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod color;
pub mod document;
pub mod object;

pub use color::{
    ColorRef, MeshPoint, MeshTriangle, PatternSpace, PatternSpec, ReflectMethod, Segment, Shading,
    from_cmyk,
};
pub use document::{
    ColorSpace, ColorSpaceKind, ColorSpaceList, CommonData, CompositeGraphicUnit,
    CompositeGraphicUnitList, DocBody, DocRoot, Document, DrawParamList, Location, Multimedia,
    MultimediaKind, MultimediaList, OfdRoot, Page, PageBlock, PageLayout, PageMode, Pages,
    Permission, PrintPermission, TabDisplay, VPreferences, ValidPeriod, Version, Zones, ZoomMode,
    ZoomSetting,
};
pub use object::{
    Action, ActionEvent, ActionKind, Actions, ClipArea, ClipList, CompositeObject, DashPattern,
    Dest, DrawParam, FillRuleStyle, GotoTarget, GraphicsUnit, ImageObject, LineCapStyle,
    LineJoinStyle, MovieAction, MovieOperator, Object, ObjectRef, PathObject, Region, RegionArea,
    SoundAction, TextClip, TextObject, UriAction,
};

/// Reference attribute (`ID` in the standard) identifying an object or resource.
pub type RefId = String;

/// Whether all required fields are present in a parsed structure.
/// OFD makes many elements optional, so validation is a separate step from parsing.
pub trait Valid {
    /// Returns `Ok(())` when the structure is internally consistent.
    fn validate(&self) -> sofd_error::Result<()>;
}
