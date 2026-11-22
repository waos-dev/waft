// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Flutter counterpart: the Android embedding's `KeyEmbedderResponder.java`, with
//! `KeyboardManager.CharacterCombiner`.
//!
//! The invariant a later editor will break: `pressing_records` holds every key the framework was
//! told went down and not yet up, so the framework's `HardwareKeyboard` never hears a key go down
//! twice or come up without going down; every event, synthesized or not, passes through
//! `update_pressing_state` before it is sent.

use std::collections::HashMap;
use std::time::Duration;

use waft_embedder::{KeyData, KeyEventDeviceType, KeyEventType};

use crate::keyboard_map::{
    ANDROID_PLANE, PRESSING_GOALS, PressingGoal, TogglingGoal, VALUE_MASK, get_toggling_goals,
    key_code_to_logical, scan_code_to_physical,
};

// Android's `KeyEvent` actions, `getAction`.
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;

// Android's `InputDevice` sources, `getSource`.
const SOURCE_DPAD: i32 = 0x0000_0201;
const SOURCE_GAMEPAD: i32 = 0x0000_0401;
const SOURCE_HDMI: i32 = 0x0200_0001;
const SOURCE_JOYSTICK: i32 = 0x0100_0010;

/// `KeyCharacterMap.VIRTUAL_KEYBOARD`: the device id of the keys an input method sends.
const VIRTUAL_KEYBOARD: i32 = -1;

/// `KeyCharacterMap.COMBINING_ACCENT` and `COMBINING_ACCENT_MASK`.
const COMBINING_ACCENT: u32 = 0x8000_0000;
const COMBINING_ACCENT_MASK: u32 = 0x7fff_ffff;

/// What the responder reads of an Android `KeyEvent`.
#[derive(Clone, Debug)]
pub(crate) struct KeyEvent {
    pub(crate) action: i32,
    pub(crate) key_code: i32,
    pub(crate) scan_code: i32,
    pub(crate) meta_state: i32,
    pub(crate) repeat_count: i32,
    pub(crate) device_id: i32,
    pub(crate) source: i32,
    /// `getEventTime`, in milliseconds.
    pub(crate) event_time: i64,
    /// `getUnicodeChar`: the character the key types with its meta state, `COMBINING_ACCENT`
    /// set for a dead key, 0 for none.
    pub(crate) unicode_char: u32,
}

/// `KeyCharacterMap.getDeadChar`: the character an accent and a character combine to, 0 for none.
pub(crate) type DeadChar<'a> = &'a dyn Fn(u32, u32) -> u32;

/// Where the responder sends each event: the framework, which answers whether it handled it.
pub(crate) type Send<'a> = &'a mut dyn FnMut(KeyData) -> bool;

/// Maps KeyEvent's action and repeatCount to a KeyData type.
fn get_event_type(event: &KeyEvent) -> KeyEventType {
    let is_repeat_event = event.repeat_count > 0;
    match event.action {
        ACTION_DOWN if is_repeat_event => KeyEventType::Repeat,
        ACTION_DOWN => KeyEventType::Down,
        ACTION_UP => KeyEventType::Up,
        _ => unreachable!("Unexpected event type"),
    }
}

pub(crate) struct KeyEmbedderResponder {
    /// The keys being pressed currently, mapped from physical keys to logical keys.
    pressing_records: HashMap<u64, u64>,
    /// Map from logical key to toggling goals.
    ///
    /// Besides immutable configuration, the toggling goals are also used to store the current
    /// enabling states in their `enabled` field.
    toggling_goals: HashMap<u64, TogglingGoal>,
    character_combiner: CharacterCombiner,
}

fn key_of_plane(key: u64, plane: u64) -> u64 {
    // Apply '& VALUE_MASK' in case the key is a negative number before being converted to long.
    plane | (key & VALUE_MASK)
}

impl KeyEmbedderResponder {
    pub(crate) fn new() -> KeyEmbedderResponder {
        let toggling_goals = get_toggling_goals()
            .into_iter()
            .map(|goal| (goal.logical_key, goal))
            .collect();
        KeyEmbedderResponder {
            pressing_records: HashMap::new(),
            toggling_goals,
            character_combiner: CharacterCombiner::default(),
        }
    }

    /// Get the physical key for this event.
    fn get_physical_key(event: &KeyEvent) -> u64 {
        let scancode = event.scan_code as u64;
        // Scancode 0 can occur during emulation using `adb shell input keyevent`. Synthesize a
        // physical key from the key code so that keys can be told apart.
        if scancode == 0 {
            // The key code can't also be 0, since those events have been filtered.
            return key_of_plane(event.key_code as u64, ANDROID_PLANE);
        }
        if let Some(by_mapping) = scan_code_to_physical(scancode) {
            return by_mapping;
        }
        key_of_plane(event.scan_code as u64, ANDROID_PLANE)
    }

    /// Get the logical key for this event.
    fn get_logical_key(event: &KeyEvent) -> u64 {
        if let Some(by_mapping) = key_code_to_logical(event.key_code as u64) {
            return by_mapping;
        }
        key_of_plane(event.key_code as u64, ANDROID_PLANE)
    }

    /// Update `pressing_records`.
    ///
    /// If the key indicated by `physical_key` is currently not pressed, then `logical_key` must
    /// not be `None` and this key will be marked pressed.
    ///
    /// If the key indicated by `physical_key` is currently pressed, then `logical_key` must be
    /// `None` and this key will be marked released.
    fn update_pressing_state(&mut self, physical_key: u64, logical_key: Option<u64>) {
        match logical_key {
            Some(logical_key) => {
                let previous_value = self.pressing_records.insert(physical_key, logical_key);
                debug_assert!(previous_value.is_none(), "The key was not empty");
            }
            None => {
                let previous_value = self.pressing_records.remove(&physical_key);
                debug_assert!(previous_value.is_some(), "The key was empty");
            }
        }
    }

    /// Synchronize for a pressing modifier (such as Shift or Ctrl).
    ///
    /// A pressing modifier is defined by a `PressingGoal`, which consists of a mask to get the
    /// true state out of `KeyEvent.getMetaState`, and a list of keys. The synchronization process
    /// dispatches synthesized events so that the state of these keys matches the true state taking
    /// the current event in consideration.
    ///
    /// Events that should be synthesized before the main event are synthesized immediately, while
    /// events that should be synthesized after the main event are appended to `post_synchronize`.
    ///
    /// Although Android KeyEvent defined bitmasks for sided modifiers (SHIFT_LEFT_ON and
    /// SHIFT_RIGHT_ON), this function only uses the unsided modifiers (SHIFT_ON), due to the weird
    /// behaviors observed on ChromeOS, where right modifiers produce events with UNSIDED |
    /// LEFT_SIDE meta state bits.
    // Java's six arguments, with where the events go.
    #[allow(clippy::too_many_arguments)]
    fn synchronize_pressing_key(
        &mut self,
        goal: &PressingGoal,
        true_pressed: bool,
        event_logical_key: u64,
        event_physical_key: u64,
        event: &KeyEvent,
        post_synchronize: &mut Vec<KeyData>,
        send: Send<'_>,
    ) {
        // During an incoming event, there might be a synthesized Flutter event for each key of
        // each pressing goal, followed by an eventual main Flutter event.
        //
        //    NowState ---------------->  PreEventState --------------> -------------->TrueState
        //              PreSynchronize                       Event      PostSynchronize
        //
        // The goal of the synchronization algorithm is to derive a pre-event state that can
        // satisfy the true state (`true_pressed`) after the event, and that requires as few
        // synthesized events based on the current state (`now_states`) as possible.
        let mut now_states = [false; 2];
        let mut pre_event_states: [Option<bool>; 2] = [None; 2];
        let mut post_event_any_pressed = false;
        // 1. Find the current states of all keys.
        // 2. Derive the pre-event state of the event key (if applicable.)
        for (key_idx, key) in goal.keys.iter().enumerate() {
            now_states[key_idx] = self.pressing_records.contains_key(&key.physical_key);
            if key.logical_key == event_logical_key {
                match get_event_type(event) {
                    KeyEventType::Down => {
                        pre_event_states[key_idx] = Some(false);
                        post_event_any_pressed = true;
                        if !true_pressed {
                            post_synchronize.push(synthesized(
                                false,
                                key.logical_key,
                                event_physical_key,
                                event.event_time,
                            ));
                        }
                    }
                    KeyEventType::Up => {
                        // Incoming event is an up. Although the previous state should be
                        // pressed, don't synthesize a down event even if it's not. The later code
                        // will handle such cases by skipping abrupt up events. Obviously don't
                        // synthesize up events either.
                        pre_event_states[key_idx] = Some(now_states[key_idx]);
                    }
                    KeyEventType::Repeat => {
                        // Incoming event is repeat. The previous state can be either pressed or
                        // released. Don't synthesize a down event here, or there will be a down
                        // event *and* a repeat event, both of which have printable characters.
                        // Obviously don't synthesize up events either.
                        if !true_pressed {
                            post_synchronize.push(synthesized(
                                false,
                                key.logical_key,
                                event_physical_key,
                                event.event_time,
                            ));
                        }
                        pre_event_states[key_idx] = Some(now_states[key_idx]);
                        post_event_any_pressed = true;
                    }
                }
            } else {
                post_event_any_pressed = post_event_any_pressed || now_states[key_idx];
            }
        }

        // Virtual keyboards (like Gboard) often keep meta bits active without sending
        // corresponding physical key events. For these, we trust the meta state and skip
        // synthesizing physical modifier keys to avoid "stuck" modifiers.
        let is_virtual_keyboard = event.device_id == VIRTUAL_KEYBOARD;

        // Fill the rest of the pre-event states to match the true state.
        if true_pressed {
            // It is required that at least one key is pressed.
            for key_idx in 0..goal.keys.len() {
                if pre_event_states[key_idx].is_some() {
                    continue;
                }
                if post_event_any_pressed || is_virtual_keyboard {
                    pre_event_states[key_idx] = Some(now_states[key_idx]);
                } else {
                    pre_event_states[key_idx] = Some(true);
                    post_event_any_pressed = true;
                }
            }
            if !post_event_any_pressed && !is_virtual_keyboard {
                pre_event_states[0] = Some(true);
            }
        } else {
            for state in &mut pre_event_states {
                if state.is_none() {
                    *state = Some(false);
                }
            }
        }

        // Dispatch synthesized events for state differences.
        for (key_idx, key) in goal.keys.iter().enumerate() {
            let pre_event_state = pre_event_states[key_idx].unwrap_or(false);
            if now_states[key_idx] != pre_event_state {
                self.synthesize_event(
                    pre_event_state,
                    key.logical_key,
                    key.physical_key,
                    event.event_time,
                    send,
                );
            }
        }
    }

    /// Synchronize for a toggling modifier (such as CapsLock).
    ///
    /// A toggling modifier is defined by a `TogglingGoal`, which consists of a mask to get the
    /// true state out of `KeyEvent.getMetaState`, and a key. The synchronization process
    /// dispatches synthesized events so that the state of these keys matches the true state taking
    /// the current event in consideration.
    ///
    /// Although Android KeyEvent defined bitmasks for all "lock" modifiers and define them as the
    /// "lock" state, weird behaviors are observed on ChromeOS. First, ScrollLock and NumLock
    /// presses do not set metaState bits. Second, CapsLock key events set the CapsLock bit as if it
    /// is a pressing modifier (key down having state 1, key up having state 0), while other key
    /// events set the CapsLock bit correctly (locked having state 1, unlocked having state 0).
    /// Therefore this function only synchronizes the CapsLock state, and only does so during
    /// non-CapsLock key events.
    fn synchronize_toggling_key(
        &mut self,
        goal_key: u64,
        true_enabled: bool,
        event_logical_key: u64,
        event: &KeyEvent,
        send: Send<'_>,
    ) {
        let Some(goal) = self.toggling_goals.get(&goal_key).cloned() else {
            return;
        };
        if goal.logical_key == event_logical_key {
            // Don't synthesize for self events, because the self events have weird metaStates on
            // ChromeOS.
            return;
        }
        if goal.enabled != true_enabled {
            let first_is_down = !self.pressing_records.contains_key(&goal.physical_key);
            if first_is_down {
                self.toggle(goal_key);
            }
            self.synthesize_event(
                first_is_down,
                goal.logical_key,
                goal.physical_key,
                event.event_time,
                send,
            );
            if !first_is_down {
                self.toggle(goal_key);
            }
            self.synthesize_event(
                !first_is_down,
                goal.logical_key,
                goal.physical_key,
                event.event_time,
                send,
            );
        }
    }

    fn toggle(&mut self, goal_key: u64) {
        if let Some(goal) = self.toggling_goals.get_mut(&goal_key) {
            goal.enabled = !goal.enabled;
        }
    }

    /// Implements the core algorithm of `handle_event`.
    ///
    /// Returns whether the framework handled the event, or `None` when no event is dispatched.
    fn handle_event_impl(
        &mut self,
        event: &KeyEvent,
        dead_char: DeadChar<'_>,
        send: Send<'_>,
    ) -> Option<bool> {
        // Events with no codes at all can not be recognized.
        if event.scan_code == 0 && event.key_code == 0 {
            return None;
        }
        let physical_key = KeyEmbedderResponder::get_physical_key(event);
        let logical_key = KeyEmbedderResponder::get_logical_key(event);

        let mut post_synchronize_events = Vec::new();
        for goal in &PRESSING_GOALS {
            self.synchronize_pressing_key(
                goal,
                (event.meta_state & goal.mask) != 0,
                logical_key,
                physical_key,
                event,
                &mut post_synchronize_events,
                send,
            );
        }

        let toggling: Vec<(u64, i32)> = self
            .toggling_goals
            .iter()
            .map(|(key, goal)| (*key, goal.mask))
            .collect();
        for (goal_key, mask) in toggling {
            self.synchronize_toggling_key(
                goal_key,
                (event.meta_state & mask) != 0,
                logical_key,
                event,
                send,
            );
        }

        let is_down_event = match event.action {
            ACTION_DOWN => true,
            ACTION_UP => false,
            _ => return None,
        };

        let event_type;
        let mut character = None;
        let last_logical_record = self.pressing_records.get(&physical_key).copied();
        if is_down_event {
            match last_logical_record {
                None => event_type = KeyEventType::Down,
                Some(last_logical_record) => {
                    // A key has been pressed that has the exact physical key as a currently
                    // pressed one.
                    if event.repeat_count > 0 {
                        event_type = KeyEventType::Repeat;
                    } else {
                        self.synthesize_event(
                            false,
                            last_logical_record,
                            physical_key,
                            event.event_time,
                            send,
                        );
                        event_type = KeyEventType::Down;
                    }
                }
            }
            let complex_char = self
                .character_combiner
                .apply_combining_character_to_base_character(event.unicode_char, dead_char);
            if complex_char != 0 {
                character = char::from_u32(complex_char).map(String::from);
            }
        } else {
            // is_down_event is false. Ignore abrupt up events.
            last_logical_record?;
            event_type = KeyEventType::Up;
        }

        if event_type != KeyEventType::Repeat {
            self.update_pressing_state(physical_key, is_down_event.then_some(logical_key));
        }
        if event_type == KeyEventType::Down {
            self.toggle(logical_key);
        }

        let device_type = match event.source {
            SOURCE_DPAD => KeyEventDeviceType::DirectionalPad,
            SOURCE_GAMEPAD => KeyEventDeviceType::Gamepad,
            SOURCE_JOYSTICK => KeyEventDeviceType::Joystick,
            SOURCE_HDMI => KeyEventDeviceType::Hdmi,
            // SOURCE_KEYBOARD among them.
            _ => KeyEventDeviceType::Keyboard,
        };

        let output = KeyData {
            time_stamp: time_stamp_of(event.event_time),
            event_type,
            device_type,
            physical: physical_key,
            logical: logical_key,
            character,
            synthesized: false,
        };

        let handled = send(output);
        for post_sync_event in post_synchronize_events {
            self.send_synthesized(post_sync_event, send);
        }
        Some(handled)
    }

    fn synthesize_event(
        &mut self,
        is_down: bool,
        logical_key: u64,
        physical_key: u64,
        timestamp: i64,
        send: Send<'_>,
    ) {
        self.send_synthesized(
            synthesized(is_down, logical_key, physical_key, timestamp),
            send,
        );
    }

    fn send_synthesized(&mut self, output: KeyData, send: Send<'_>) {
        if output.physical != 0 && output.logical != 0 {
            let is_down = output.event_type == KeyEventType::Down;
            self.update_pressing_state(output.physical, is_down.then_some(output.logical));
        }
        send(output);
    }

    /// Parses an Android key event, performs synchronization, and dispatches Flutter events to the
    /// framework through `send`, answering whether the framework handled the event. An event the
    /// responder cannot read, or an up whose key the framework never heard go down, is not sent,
    /// and is answered as not handled, so it goes on to the views as if Waft had none.
    pub(crate) fn handle_event(
        &mut self,
        event: &KeyEvent,
        dead_char: DeadChar<'_>,
        send: Send<'_>,
    ) -> bool {
        self.handle_event_impl(event, dead_char, send)
            .unwrap_or(false)
    }
}

/// A synthesized event: a key going down or up that no Android event says, so the framework's
/// state matches what the keyboard holds.
fn synthesized(is_down: bool, logical_key: u64, physical_key: u64, timestamp: i64) -> KeyData {
    KeyData {
        time_stamp: time_stamp_of(timestamp),
        event_type: if is_down {
            KeyEventType::Down
        } else {
            KeyEventType::Up
        },
        device_type: KeyEventDeviceType::Keyboard,
        physical: physical_key,
        logical: logical_key,
        character: None,
        synthesized: true,
    }
}

fn time_stamp_of(event_time: i64) -> Duration {
    Duration::from_millis(u64::try_from(event_time).unwrap_or(0))
}

/// `KeyboardManager.CharacterCombiner`: applies a combining character (a dead key's accent) to
/// the next character typed.
#[derive(Default)]
pub(crate) struct CharacterCombiner {
    combining_character: u32,
}

impl CharacterCombiner {
    /// This method mutates `combining_character` over time to combine characters.
    ///
    /// One of the following things happens in this method:
    ///
    /// - If no previous `combining_character` exists and the `new_character_code_point` is not a
    ///   combining character, then `new_character_code_point` is returned.
    /// - If no previous `combining_character` exists and the `new_character_code_point` is a
    ///   combining character, then `new_character_code_point` is saved as the
    ///   `combining_character` and 0 is returned.
    /// - If a previous `combining_character` exists and the `new_character_code_point` is also a
    ///   combining character, then the `new_character_code_point` is combined with the existing
    ///   `combining_character` and 0 is returned.
    /// - If a previous `combining_character` exists and the `new_character_code_point` is not a
    ///   combining character, then the `combining_character` is applied to the regular
    ///   `new_character_code_point` and the resulting complex character is returned. The
    ///   `combining_character` is cleared.
    ///
    /// The following reference explains the concept of a "combining character":
    /// https://en.wikipedia.org/wiki/Combining_character
    pub(crate) fn apply_combining_character_to_base_character(
        &mut self,
        new_character_code_point: u32,
        dead_char: DeadChar<'_>,
    ) -> u32 {
        let mut complex_character = new_character_code_point & 0xffff;
        let is_new_code_point_a_combining_character =
            (new_character_code_point & COMBINING_ACCENT) != 0;
        if is_new_code_point_a_combining_character {
            // If a combining character was entered before, combine this one with that one.
            let plain_code_point = new_character_code_point & COMBINING_ACCENT_MASK;
            if self.combining_character != 0 {
                self.combining_character = dead_char(self.combining_character, plain_code_point);
            } else {
                self.combining_character = plain_code_point;
            }
        } else {
            // The new character is a regular character. Apply combining_character to it, if it
            // exists.
            if self.combining_character != 0 {
                let combined_char = dead_char(self.combining_character, new_character_code_point);
                if combined_char > 0 {
                    complex_character = combined_char & 0xffff;
                }
                self.combining_character = 0;
            }
        }
        complex_character
    }
}

#[cfg(test)]
mod tests {
    //! Cases of Flutter's `KeyboardManagerTest.java` for the embedder responder.

    use super::*;

    const SCAN_KEY_A: i32 = 0x1e;
    const SCAN_SHIFT_LEFT: i32 = 0x2a;
    const SCAN_ARROW_LEFT: i32 = 0x69;
    const SCAN_ARROW_RIGHT: i32 = 0x6a;

    const KEYCODE_A: i32 = 29;
    const KEYCODE_SHIFT_LEFT: i32 = 59;
    const KEYCODE_ENTER: i32 = 66;
    const KEYCODE_DPAD_LEFT: i32 = 21;

    const PHYSICAL_KEY_A: u64 = 0x0007_0004;
    const PHYSICAL_SHIFT_LEFT: u64 = 0x0007_00e1;
    const PHYSICAL_ARROW_LEFT: u64 = 0x0007_0050;
    const PHYSICAL_ARROW_RIGHT: u64 = 0x0007_004f;

    const LOGICAL_KEY_A: u64 = 0x61;
    const LOGICAL_SHIFT_LEFT: u64 = 0x02_0000_0102;
    const LOGICAL_ENTER: u64 = 0x01_0000_000d;

    const META_SHIFT_ON: i32 = 0x1;
    const META_SHIFT_LEFT_ON: i32 = 0x40;

    const DEAD_KEY: u32 = '`' as u32 | COMBINING_ACCENT;

    /// Flutter's `FakeKeyEvent(action, scanCode, keyCode, repeatCount, character, metaState)`.
    fn key(
        action: i32,
        scan_code: i32,
        key_code: i32,
        repeat_count: i32,
        character: char,
        meta_state: i32,
    ) -> KeyEvent {
        KeyEvent {
            action,
            key_code,
            scan_code,
            meta_state,
            repeat_count,
            device_id: 0,
            source: 0x0000_0101, // SOURCE_KEYBOARD
            event_time: 0,
            unicode_char: character as u32,
        }
    }

    /// `KeyCharacterMap.getDeadChar` for the grave accent alone.
    fn dead_char(accent: u32, character: u32) -> u32 {
        match (char::from_u32(accent), char::from_u32(character)) {
            (Some('`'), Some('A')) => 'À' as u32,
            (Some('`'), Some('`')) => '`' as u32,
            _ => 0,
        }
    }

    /// What one event sends to a framework that handles everything, and the answer.
    fn send(responder: &mut KeyEmbedderResponder, event: KeyEvent) -> (bool, Vec<KeyData>) {
        let mut sent = Vec::new();
        let handled = responder.handle_event(&event, &dead_char, &mut |data| {
            sent.push(data);
            true
        });
        (handled, sent)
    }

    fn data(
        event_type: KeyEventType,
        physical: u64,
        logical: u64,
        character: Option<&str>,
        synthesized: bool,
    ) -> KeyData {
        KeyData {
            time_stamp: Duration::ZERO,
            event_type,
            device_type: KeyEventDeviceType::Keyboard,
            physical,
            logical,
            character: character.map(String::from),
            synthesized,
        }
    }

    #[test]
    fn tap_lower_a() {
        let mut responder = KeyEmbedderResponder::new();
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, SCAN_KEY_A, KEYCODE_A, 0, 'a', 0)
            ),
            (
                true,
                vec![data(
                    KeyEventType::Down,
                    PHYSICAL_KEY_A,
                    LOGICAL_KEY_A,
                    Some("a"),
                    false
                )]
            )
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, SCAN_KEY_A, KEYCODE_A, 1, 'a', 0)
            ),
            (
                true,
                vec![data(
                    KeyEventType::Repeat,
                    PHYSICAL_KEY_A,
                    LOGICAL_KEY_A,
                    Some("a"),
                    false
                )]
            )
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_UP, SCAN_KEY_A, KEYCODE_A, 0, 'a', 0)
            ),
            (
                true,
                vec![data(
                    KeyEventType::Up,
                    PHYSICAL_KEY_A,
                    LOGICAL_KEY_A,
                    None,
                    false
                )]
            )
        );
    }

    #[test]
    fn tap_upper_a() {
        let mut responder = KeyEmbedderResponder::new();
        assert_eq!(
            send(
                &mut responder,
                key(
                    ACTION_DOWN,
                    SCAN_SHIFT_LEFT,
                    KEYCODE_SHIFT_LEFT,
                    0,
                    '\0',
                    0x41
                )
            )
            .1,
            vec![data(
                KeyEventType::Down,
                PHYSICAL_SHIFT_LEFT,
                LOGICAL_SHIFT_LEFT,
                None,
                false
            )]
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, SCAN_KEY_A, KEYCODE_A, 0, 'A', 0x41)
            )
            .1,
            vec![data(
                KeyEventType::Down,
                PHYSICAL_KEY_A,
                LOGICAL_KEY_A,
                Some("A"),
                false
            )]
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_UP, SCAN_KEY_A, KEYCODE_A, 0, 'A', 0x41)
            )
            .1,
            vec![data(
                KeyEventType::Up,
                PHYSICAL_KEY_A,
                LOGICAL_KEY_A,
                None,
                false
            )]
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_UP, SCAN_SHIFT_LEFT, KEYCODE_SHIFT_LEFT, 0, '\0', 0)
            )
            .1,
            vec![data(
                KeyEventType::Up,
                PHYSICAL_SHIFT_LEFT,
                LOGICAL_SHIFT_LEFT,
                None,
                false
            )]
        );
    }

    #[test]
    fn events_with_minted_codes() {
        let mut responder = KeyEmbedderResponder::new();
        // Zero scan code.
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, 0, KEYCODE_ENTER, 0, '\n', 0)
            )
            .1,
            vec![data(
                KeyEventType::Down,
                0x11_0000_0042,
                LOGICAL_ENTER,
                Some("\n"),
                false
            )]
        );
        // Zero scan code and zero key code: not sent, and not handled.
        assert_eq!(
            send(&mut responder, key(ACTION_DOWN, 0, 0, 0, '\0', 0)),
            (false, vec![])
        );
        // Unrecognized scan code.
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, 0xDEADBEEF_u32 as i32, 0, 0, '\0', 0)
            )
            .1,
            vec![data(
                KeyEventType::Down,
                0x11_DEAD_BEEF,
                0x11_0000_0000,
                None,
                false
            )]
        );
        // Zero key code.
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, SCAN_ARROW_LEFT, 0, 0, '\0', 0)
            )
            .1,
            vec![data(
                KeyEventType::Down,
                PHYSICAL_ARROW_LEFT,
                0x11_0000_0000,
                None,
                false
            )]
        );
        // Unrecognized key code.
        assert_eq!(
            send(
                &mut responder,
                key(
                    ACTION_DOWN,
                    SCAN_ARROW_RIGHT,
                    0xDEADBEEF_u32 as i32,
                    0,
                    '\0',
                    0
                )
            )
            .1,
            vec![data(
                KeyEventType::Down,
                PHYSICAL_ARROW_RIGHT,
                0x11_DEAD_BEEF,
                None,
                false
            )]
        );
    }

    #[test]
    fn duplicate_down_events_are_preceded_by_synthesized_up_events() {
        let mut responder = KeyEmbedderResponder::new();
        send(
            &mut responder,
            key(ACTION_DOWN, SCAN_KEY_A, KEYCODE_A, 0, 'a', 0),
        );
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_DOWN, SCAN_KEY_A, KEYCODE_A, 0, 'a', 0)
            )
            .1,
            vec![
                data(KeyEventType::Up, PHYSICAL_KEY_A, LOGICAL_KEY_A, None, true),
                data(
                    KeyEventType::Down,
                    PHYSICAL_KEY_A,
                    LOGICAL_KEY_A,
                    Some("a"),
                    false
                ),
            ]
        );
    }

    #[test]
    fn abrupt_up_events_are_ignored() {
        let mut responder = KeyEmbedderResponder::new();
        assert_eq!(
            send(
                &mut responder,
                key(ACTION_UP, SCAN_KEY_A, KEYCODE_A, 0, 'a', 0)
            ),
            (false, vec![])
        );
    }

    #[test]
    fn synchronize_shift_left_during_foreign_key_events() {
        let mut responder = KeyEmbedderResponder::new();
        let shift_left_on = META_SHIFT_LEFT_ON | META_SHIFT_ON;
        let (_, sent) = send(
            &mut responder,
            key(
                ACTION_DOWN,
                SCAN_ARROW_LEFT,
                KEYCODE_DPAD_LEFT,
                0,
                '\0',
                shift_left_on,
            ),
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[0],
            data(
                KeyEventType::Down,
                PHYSICAL_SHIFT_LEFT,
                LOGICAL_SHIFT_LEFT,
                None,
                true
            )
        );
        let (_, sent) = send(
            &mut responder,
            key(ACTION_UP, SCAN_ARROW_LEFT, KEYCODE_DPAD_LEFT, 0, '\0', 0),
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[0],
            data(
                KeyEventType::Up,
                PHYSICAL_SHIFT_LEFT,
                LOGICAL_SHIFT_LEFT,
                None,
                true
            )
        );
    }

    #[test]
    fn a_virtual_keyboard_s_meta_state_synthesizes_no_modifier_key() {
        let mut responder = KeyEmbedderResponder::new();
        let mut event = key(
            ACTION_DOWN,
            SCAN_ARROW_LEFT,
            KEYCODE_DPAD_LEFT,
            0,
            '\0',
            META_SHIFT_ON,
        );
        event.device_id = VIRTUAL_KEYBOARD;
        let (_, sent) = send(&mut responder, event);
        assert_eq!(
            sent,
            vec![data(
                KeyEventType::Down,
                PHYSICAL_ARROW_LEFT,
                0x01_0000_0302,
                None,
                false
            )]
        );
    }

    #[test]
    fn basic_combing_characters_test() {
        let mut combiner = CharacterCombiner::default();
        let mut apply = |code_point: u32| {
            combiner.apply_combining_character_to_base_character(code_point, &dead_char)
        };
        assert_eq!(apply(0), 0);
        assert_eq!(apply('B' as u32), 'B' as u32);
        assert_eq!(apply('B' as u32), 'B' as u32);
        assert_eq!(apply('A' as u32), 'A' as u32);
        assert_eq!(apply(0), 0);
        assert_eq!(apply(0), 0);

        assert_eq!(apply(DEAD_KEY), '`' as u32);
        assert_eq!(apply(DEAD_KEY), '`' as u32);
        assert_eq!(apply('A' as u32), 'À' as u32);

        assert_eq!(apply(DEAD_KEY), '`' as u32);
        assert_eq!(apply(0), 0);
        // The 0 input should remove the combining state.
        assert_eq!(apply('A' as u32), 'A' as u32);

        assert_eq!(apply(0), 0);
        assert_eq!(apply(DEAD_KEY), '`' as u32);
        assert_eq!(apply('A' as u32), 'À' as u32);
    }
}
