//! Platform-independent logic for nouwall: configuration, wallpaper manifests,
//! hardware tier classification, warning policy, pause decisions and the
//! frame-time governor. Everything here is pure and unit-tested on any OS.

pub mod config;
pub mod governor;
pub mod hardware;
pub mod library;
pub mod manifest;
pub mod pause;
pub mod policy;
pub mod tier;

pub use config::{Assignment, Config, GeneralSettings, MonitorTarget};
pub use governor::Governor;
pub use hardware::{Codec, DecodeCaps, GpuInfo, GpuKind, HardwareInfo, MonitorInfo, PowerSource};
pub use manifest::{Kind, Manifest, VideoMeta};
pub use library::{Entry, Library, LibraryError};
pub use pause::{Coverage, PauseInputs, PauseReason};
pub use policy::{Severity, Warning, WarningCode};
pub use tier::{Budget, Profiles, Tier, TierReport};
