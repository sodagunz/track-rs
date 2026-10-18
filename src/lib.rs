#[cfg(test)]
mod arrangement_tests;
pub mod audio;
#[cfg(test)]
mod audition_tests;
pub mod bindings;
#[cfg(test)]
mod budget_tests;
mod clipboard;
#[cfg(test)]
mod clipboard_tests;
pub mod command;
pub mod history;
#[cfg(test)]
mod history_tests;
pub mod midi;
#[cfg(test)]
mod playback_tests;
pub mod state;
pub mod synth;
#[cfg(test)]
mod synth_tracker_tests;
pub mod theme;
#[cfg(test)]
mod timing_integration_tests;
#[cfg(test)]
mod track_tests;
pub mod tracker;
pub mod transport;
pub mod ui;

pub mod project;
pub mod project_file;
mod quantize;
#[cfg(test)]
mod quantize_tests;

#[cfg(test)]
mod project_tests;

#[cfg(test)]
mod length_tests;

#[cfg(test)]
mod mixer_tests;

#[cfg(test)]
mod resize_tests;

#[cfg(test)]
mod count_in_tests;
#[cfg(test)]
mod grid_tests;
pub mod meter;
#[cfg(test)]
mod metronome_tests;
pub mod musical_time;
pub mod recording;
#[cfg(test)]
mod recording_tests;
mod tone;
