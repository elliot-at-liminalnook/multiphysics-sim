//! In-process hardware application services. Hosts own execution workers;
//! safety signals remain independently accessible from ordinary acquisition.
pub mod bench;
pub mod calibration;
pub mod local;
pub mod ownership;
pub mod protocol;
