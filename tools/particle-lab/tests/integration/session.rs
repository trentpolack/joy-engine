// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Session behavior without a device: Live application with debounce, invalid drafts kept apart from the accepted
//! preview, property undo, recovery storage, playback and seeking, texture uploads, monitoring, and the hosting
//! contract.

use joy_engine::development::{
    AuthoringTool,
    ToolDocument,
    ToolHosting,
};
use joy_engine::particles::{
    DEFAULT_PARTICLE_TEXTURE,
    ParticleParameter,
    ParticleRenderer,
};
use joy_engine::runtime::Storage;
use particle_lab::ParticleLabTool;
use particle_lab::constants::{
    EmitterPreset,
    PerformanceMode,
    STORAGE_KEY,
};
use particle_lab::document::write_asset;
use particle_lab::examples::copy_example;
use particle_lab::lab::{
    ParticleLab,
    PreviewState,
    ReplayWindow,
};

const FRAME: f64 = 1.0 / 60.0;

fn run(lab: &mut ParticleLab, seconds: f64) {
    let frames = (seconds / FRAME).round() as usize;
    for _ in 0..frames {
        lab.frame(FRAME);
    }
}

fn power(lab: &mut ParticleLab) -> &mut f64 {
    match lab.asset_mut().parameters.get_mut("power") {
        Some(ParticleParameter::Scalar { value, .. }) => value,
        _ => panic!("Ember Impact has a scalar power"),
    }
}

fn temporary_storage(name: &str) -> (Storage, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!("particle-lab-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    (Storage::in_directory(&directory), directory)
}

#[test]
fn live_applies_after_the_debounce_and_newer_edits_restart_it() {
    let mut lab = ParticleLab::new(None);
    let first = lab.definition().unwrap().clone();
    *power(&mut lab) = 1.5;
    lab.document_changed();
    assert_eq!(lab.preview_state(), PreviewState::Pending);
    run(&mut lab, 0.15);
    *power(&mut lab) = 1.6;
    lab.document_changed();
    run(&mut lab, 0.2);
    assert_eq!(lab.preview_state(), PreviewState::Pending, "the second edit restarted the delay");
    run(&mut lab, 0.1);
    assert_eq!(lab.preview_state(), PreviewState::Live);
    assert!(!lab.definition().unwrap().ptr_eq(&first));

    // Live off cancels the queue; Live on applies pending edits after the delay.
    *power(&mut lab) = 1.7;
    lab.document_changed();
    lab.set_live(false);
    assert!(!lab.apply_queued());
    run(&mut lab, 1.0);
    assert_eq!(lab.preview_state(), PreviewState::Pending);
    assert!(!lab.apply(true), "automatic applies obey Live");
    lab.set_live(true);
    run(&mut lab, 0.3);
    assert_eq!(lab.preview_state(), PreviewState::Live);
}

#[test]
fn invalid_script_drafts_never_replace_the_accepted_preview() {
    let mut lab = ParticleLab::new(None);
    let accepted = lab.definition().unwrap().clone();
    lab.asset_mut().emitters[0].spawn = "size = (".to_owned();
    lab.script_changed();
    run(&mut lab, 0.4);
    assert!(lab.definition().unwrap().ptr_eq(&accepted), "the accepted preview stays");
    assert!(lab.error().unwrap().contains("Emitter 'flash'"), "{:?}", lab.error());
    assert_eq!(lab.preview_state(), PreviewState::Pending);
    let (spawn, update) = lab.script_diagnostics();
    assert_eq!(spawn.len(), 1);
    assert_eq!(spawn[0].line, 1);
    assert!(update.is_empty());
    assert!(lab.export_text().is_ok(), "drafts with script errors still export");

    lab.asset_mut().emitters[0].spawn = "size = 4;".to_owned();
    lab.script_changed();
    run(&mut lab, 0.4);
    assert!(lab.error().is_none());
    assert_eq!(lab.preview_state(), PreviewState::Live);
}

#[test]
fn property_undo_restores_values_and_scripts_or_structure_end_its_reach() {
    let mut lab = ParticleLab::new(None);
    *power(&mut lab) = 1.5;
    lab.property_committed();
    *power(&mut lab) = 1.75;
    lab.property_committed();
    lab.undo();
    assert_eq!(*power(&mut lab), 1.5);
    lab.redo();
    assert_eq!(*power(&mut lab), 1.75);
    lab.undo();
    lab.undo();
    assert_eq!(*power(&mut lab), 1.0);
    assert!(!lab.can_undo() && lab.can_redo());

    // A script edit starts a new property history.
    lab.redo();
    lab.asset_mut().emitters[0].update.push_str(" // tuned");
    lab.script_changed();
    assert!(!lab.can_undo() && !lab.can_redo());

    // A structural edit leaves earlier entries, but undoing past it clears them instead of restoring over it.
    *power(&mut lab) = 0.5;
    lab.property_committed();
    lab.add_parameter().unwrap();
    lab.undo();
    assert_eq!(*power(&mut lab), 0.5, "the structural edit is never overwritten");
    assert!(!lab.can_undo());
    lab.add_emitter(EmitterPreset::Ring).unwrap();
    assert_eq!(lab.asset().emitters.last().unwrap().renderer, ParticleRenderer::Ring);
    assert_eq!(lab.current_emitter(), lab.asset().emitters.len() - 1, "new emitters are selected");
}

#[test]
fn recovery_restores_unfinished_drafts_and_hosted_sessions_write_nothing() {
    let (storage, directory) = temporary_storage("recovery");
    let mut lab = ParticleLab::new(Some(storage.clone()));
    lab.asset_mut().name = "Recovered Draft".to_owned();
    lab.asset_mut().emitters[1].update = "velocity.y -= (".to_owned();
    lab.script_changed();
    run(&mut lab, 0.6);
    let saved = storage.load(STORAGE_KEY).unwrap().expect("the draft is saved");
    assert!(saved.contains("Recovered Draft"));
    let restored = ParticleLab::new(Some(storage.clone()));
    assert_eq!(restored.asset().name, "Recovered Draft");
    assert_eq!(restored.asset().emitters[1].update, "velocity.y -= (", "unfinished scripts survive recovery");

    storage.remove(STORAGE_KEY).unwrap();
    let mut hosted = ParticleLab::new(None);
    hosted.asset_mut().name = "Hosted".to_owned();
    hosted.document_changed();
    run(&mut hosted, 0.6);
    assert_eq!(storage.load(STORAGE_KEY).unwrap(), None);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn seeking_runs_incrementally_and_resumes_playback() {
    let mut lab = ParticleLab::new(None);
    lab.seek(5.0);
    assert!(lab.seeking() && !lab.playing());
    lab.frame(FRAME);
    lab.frame(FRAME);
    assert!(!lab.seeking());
    assert!((lab.time() - 5.0).abs() < 1e-9, "{}", lab.time());
    assert!(lab.playing(), "playback resumes after the seek");

    // A newer action supersedes a running seek.
    lab.seek(20.0);
    lab.step();
    assert!(!lab.seeking() && !lab.playing());

    // The replay window stops playback at its end without Loop and restarts with it.
    lab.looping = false;
    lab.set_replay_window(ReplayWindow::Seconds(0.5)).unwrap();
    run(&mut lab, 0.7);
    assert!(!lab.playing());
    assert!(lab.set_replay_window(ReplayWindow::Seconds(0.0)).is_err());
    assert!(lab.set_replay_window(ReplayWindow::Seconds(31.0)).is_err());
    lab.looping = true;
    lab.set_replay_window(ReplayWindow::Seconds(0.5)).unwrap();
    run(&mut lab, 0.7);
    assert!(lab.playing() && lab.time() < 0.5, "looping restarts the window: {}", lab.time());
    assert_eq!(lab.timeline_seconds(), 0.5);
}

#[test]
fn texture_uploads_validate_pngs_and_monitoring_profiles_per_emitter() {
    let mut lab = ParticleLab::new(None);
    lab.load_example(2).unwrap();
    let textured = lab.asset().emitters.iter().position(|emitter| emitter.renderer.is_textured()).unwrap();
    lab.select_emitter(textured).unwrap();
    let payload = DEFAULT_PARTICLE_TEXTURE.strip_prefix("data:image/png;base64,").unwrap();
    let png = base64(payload);
    lab.upload_texture(&png).unwrap();
    assert_eq!(lab.asset().emitters[textured].texture.as_ref().unwrap().source.as_ref(), DEFAULT_PARTICLE_TEXTURE);
    assert_eq!(lab.upload_texture(b"GIF89a"), Err("Choose a PNG texture up to 1 MiB.".to_owned()));
    assert_eq!(lab.upload_texture(&vec![0; 1024 * 1024 + 1]), Err("Choose a PNG texture up to 1 MiB.".to_owned()));
    run(&mut lab, 0.4);
    assert_eq!(lab.preview_state(), PreviewState::Live, "the uploaded texture applies");
    assert_eq!(
        lab.viewport.textures().len(),
        lab.definition().map_or(0, |definition| {
            definition
                .emitters()
                .iter()
                .filter_map(|emitter| emitter.asset.texture.as_ref().map(|texture| texture.source.clone()))
                .collect::<std::collections::HashSet<_>>()
                .len()
        })
    );

    lab.set_performance_mode(PerformanceMode::Detailed);
    run(&mut lab, 0.6);
    let rows = &lab.performance().emitters;
    assert_eq!(rows.len(), lab.asset().emitters.len());
    assert!(rows.iter().any(|row| row.spawned > 0 && row.operations > 0), "{rows:?}");
    lab.set_performance_mode(PerformanceMode::Disabled);
    assert!(!lab.effect().unwrap().profile().enabled);
}

#[test]
fn the_hosting_contract_opens_saves_and_tracks_changes() {
    let mut tool = ParticleLabTool::with_storage(None);
    assert_eq!(tool.tool_name(), "PARTICLE LAB");
    assert_eq!(tool.document_extensions(), ["joyfx"]);
    let before = tool.lab().asset().clone();
    let refused =
        tool.open_document(ToolDocument { name: "broken.joyfx".to_owned(), contents: "{\"version\": 2}".to_owned() });
    assert!(refused.is_err());
    assert_eq!(tool.lab().asset(), &before, "a refused document leaves the current one");

    let solar = write_asset(&copy_example(1));
    tool.open_document(ToolDocument { name: "solar.joyfx".to_owned(), contents: solar.clone() }).unwrap();
    assert_eq!(tool.lab().asset().name, "Solar Swirl");
    assert!(!tool.has_unsaved_changes());
    tool.lab_mut().asset_mut().name = "Solar Swirl Two".to_owned();
    tool.lab_mut().document_changed();
    assert!(tool.has_unsaved_changes());
    let saved = tool.save_document("solar.joyfx").unwrap();
    assert!(saved.contains("Solar Swirl Two"));
    tool.mark_saved();
    assert!(!tool.has_unsaved_changes());
    tool.set_hosting(ToolHosting::Hosted {
        area: joy_engine::egui::Rect::from_min_size(joy_engine::egui::Pos2::ZERO, joy_engine::egui::vec2(900.0, 600.0)),
    });
    tool.set_hosting(ToolHosting::Standalone);
}

fn base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let mut output = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let padding = chunk.iter().filter(|byte| **byte == b'=').count();
        let bits = chunk.iter().filter(|byte| **byte != b'=').fold(0u32, |bits, byte| (bits << 6) | value(*byte))
            << (6 * padding);
        output.extend_from_slice(&[(bits >> 16) as u8, (bits >> 8) as u8, bits as u8][..3 - padding]);
    }
    output
}
