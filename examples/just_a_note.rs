use track_rs::{
    audio::AudioEngine,
    tracker::{Note, NotePitch},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut au = AudioEngine::new()?;
    let note = Note {
        pitch: NotePitch::E,
        octave: 3,
    };
    au.play(note);

    std::thread::sleep(std::time::Duration::from_secs(1));

    Ok(())
}
