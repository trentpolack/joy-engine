// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser audio output through Web Audio: one context, one buffer per sound, and a source and gain node per voice,
//! mirroring the TypeScript `AudioBank`.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioScheduledSourceNode, GainNode,
};

use crate::runtime::audio::{DecodedSound, VoiceRecord, voices_to_steal};

/// Page events that count as user activation and resume a suspended context.
const UNLOCK_EVENTS: [&str; 3] = ["pointerdown", "keydown", "touchend"];

/// A page event listener that resumes the context, kept so it can be removed.
type UnlockListener = (&'static str, Closure<dyn FnMut()>);

/// One playing voice and the callback that reports its natural end.
struct WebVoice {
    id: u64,
    sound: usize,
    source: AudioBufferSourceNode,
    gain: GainNode,
    _on_ended: Closure<dyn FnMut()>,
}

/// An owned Web Audio context, or none when the browser cannot create one.
pub(crate) struct WebOutput {
    context: Option<AudioContext>,
    buffers: Vec<AudioBuffer>,
    /// Oldest first.
    voices: Vec<WebVoice>,
    /// Voices whose sources ended naturally, retired outside the callback that reports them.
    ended: Rc<RefCell<Vec<u64>>>,
    next_id: u64,
    unlock: Vec<UnlockListener>,
}

impl WebOutput {
    /// Create the context and a buffer per sound, and resume on the page's next gesture. A missing context is logged
    /// and leaves a silent output; a buffer the browser rejects is reported with its sound's index.
    pub(crate) fn new(sounds: &[(String, DecodedSound)]) -> Result<Self, (usize, String)> {
        let mut output = Self {
            context: None,
            buffers: Vec::new(),
            voices: Vec::new(),
            ended: Rc::default(),
            next_id: 0,
            unlock: Vec::new(),
        };
        let options = web_sys::AudioContextOptions::new();
        let context = match AudioContext::new_with_context_options(&options) {
            Ok(context) => context,
            Err(error) => {
                log::warn!("Audio output is unavailable: {}", describe(&error));
                return Ok(output);
            }
        };
        for (index, (_, sound)) in sounds.iter().enumerate() {
            output.buffers.push(create_buffer(&context, sound).map_err(|error| (index, describe(&error)))?);
        }
        if let Some(window) = web_sys::window() {
            for event in UNLOCK_EVENTS {
                let target = context.clone();
                let listener: Closure<dyn FnMut()> = Closure::new(move || {
                    if target.state() == AudioContextState::Suspended {
                        let _ = target.resume();
                    }
                });
                if window.add_event_listener_with_callback(event, listener.as_ref().unchecked_ref()).is_ok() {
                    output.unlock.push((event, listener));
                }
            }
        }
        output.context = Some(context);
        Ok(output)
    }

    pub(crate) fn running(&self) -> bool {
        self.context.as_ref().is_some_and(|context| context.state() == AudioContextState::Running)
    }

    pub(crate) fn resume(&mut self) -> bool {
        let Some(context) = &self.context else {
            return false;
        };
        match context.state() {
            AudioContextState::Running => true,
            AudioContextState::Closed => false,
            _ => {
                // A denied request may stay pending; later gestures retry rather than waiting on it.
                let _ = context.resume();
                context.state() == AudioContextState::Running
            }
        }
    }

    pub(crate) fn play(&mut self, sound: usize, volume: f32, sound_limit: usize, bank_limit: usize) -> bool {
        self.reap();
        let records: Vec<VoiceRecord> =
            self.voices.iter().map(|voice| VoiceRecord { id: voice.id, sound: voice.sound }).collect();
        for id in voices_to_steal(&records, sound, sound_limit, bank_limit) {
            if let Some(index) = self.voices.iter().position(|voice| voice.id == id) {
                release(self.voices.remove(index), true);
            }
        }
        match self.start(sound, volume) {
            Ok(voice) => {
                self.voices.push(voice);
                true
            }
            Err(error) => {
                log::warn!("A sound could not start: {}", describe(&error));
                false
            }
        }
    }

    pub(crate) fn active_voice_count(&mut self) -> usize {
        self.reap();
        self.voices.len()
    }

    pub(crate) fn close(&mut self) {
        for voice in self.voices.drain(..) {
            release(voice, true);
        }
        self.ended.borrow_mut().clear();
        if let Some(window) = web_sys::window() {
            for (event, listener) in self.unlock.drain(..) {
                let _ = window.remove_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
            }
        }
        self.buffers.clear();
        if let Some(context) = self.context.take() {
            let _ = context.close();
        }
    }

    /// Start a new source through its own gain node; sources are single-use in Web Audio.
    fn start(&mut self, sound: usize, volume: f32) -> Result<WebVoice, wasm_bindgen::JsValue> {
        let (Some(context), Some(buffer)) = (&self.context, self.buffers.get(sound)) else {
            return Err("audio output is closed".into());
        };
        let source = context.create_buffer_source()?;
        let gain = context.create_gain()?;
        source.set_buffer(Some(buffer));
        gain.gain().set_value(volume);
        source.connect_with_audio_node(&gain)?;
        gain.connect_with_audio_node(&context.destination())?;
        let id = self.next_id;
        self.next_id += 1;
        let ended = self.ended.clone();
        let on_ended: Closure<dyn FnMut()> = Closure::new(move || ended.borrow_mut().push(id));
        // The scheduling calls live on the base node type; web-sys deprecates the subclass copies.
        let scheduled: &AudioScheduledSourceNode = &source;
        scheduled.set_onended(Some(on_ended.as_ref().unchecked_ref()));
        scheduled.start()?;
        Ok(WebVoice { id, sound, source, gain, _on_ended: on_ended })
    }

    /// Retire voices whose sources ended since the last call.
    fn reap(&mut self) {
        let ended = std::mem::take(&mut *self.ended.borrow_mut());
        for id in ended {
            if let Some(index) = self.voices.iter().position(|voice| voice.id == id) {
                release(self.voices.remove(index), false);
            }
        }
    }
}

impl Drop for WebOutput {
    fn drop(&mut self) {
        self.close();
    }
}

/// Disconnect a voice exactly once, stopping it first when it is still playing.
fn release(voice: WebVoice, stop: bool) {
    let scheduled: &AudioScheduledSourceNode = &voice.source;
    scheduled.set_onended(None);
    if stop {
        let _ = scheduled.stop();
    }
    let _ = voice.source.disconnect();
    let _ = voice.gain.disconnect();
}

/// A stereo buffer holding one decoded sound.
fn create_buffer(context: &AudioContext, sound: &DecodedSound) -> Result<AudioBuffer, wasm_bindgen::JsValue> {
    let length = u32::try_from(sound.frames.len()).map_err(|_| "the sound is too long")?;
    let buffer = context.create_buffer(2, length, sound.sample_rate as f32)?;
    for channel in 0..2 {
        let samples: Vec<f32> = sound.frames.iter().map(|frame| frame[channel]).collect();
        buffer.copy_to_channel(&samples, channel as i32)?;
    }
    Ok(buffer)
}

/// A readable message for a JavaScript exception.
fn describe(error: &wasm_bindgen::JsValue) -> String {
    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}
