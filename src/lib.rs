#[cfg(test)]
mod arrangement_tests;
pub mod audio;
#[cfg(test)]
mod audition_tests;
pub mod bindings;
#[cfg(test)]
mod clipboard_tests;
pub mod command;
pub mod history;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod playback_tests;
pub mod state;
#[cfg(test)]
mod track_tests;
pub mod tracker;
pub mod transport;
pub mod ui;

pub mod project;
pub mod project_file;

#[cfg(test)]
mod project_tests;

#[cfg(test)]
mod length_tests;

#[cfg(test)]
mod mixer_tests;

pub mod meter;
mod tone;
