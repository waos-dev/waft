// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Flutter counterpart: the Android embedding's `KeyboardMap.java`.
//!
//! Transcribed from that file, which Flutter itself generates from `dev/tools/gen_keycodes`. The two
//! tables at the end are machine-written from it; edit the generator, not them.

// Android's `KeyEvent` meta state bits, `getMetaState`.
const META_SHIFT_ON: i32 = 0x1;
const META_ALT_ON: i32 = 0x2;
const META_CTRL_ON: i32 = 0x1000;
const META_CAPS_LOCK_ON: i32 = 0x0010_0000;

/// A physicalKey-logicalKey pair used to define mappings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyPair {
    pub(crate) physical_key: u64,
    pub(crate) logical_key: u64,
}

/// An immutable configuration item that defines how to synchronize pressing modifiers (such as
/// Shift or Ctrl), so that the `KeyEmbedderResponder` must synthesize events until the combined
/// pressing state of `keys` matches the true meta state masked by `mask`.
pub(crate) struct PressingGoal {
    pub(crate) mask: i32,
    pub(crate) keys: [KeyPair; 2],
}

/// A configuration item that defines how to synchronize toggling modifiers (such as CapsLock), so
/// that the `KeyEmbedderResponder` must synthesize events until the enabling state of the key
/// matches the true meta state masked by `mask`.
///
/// The objects of this class are mutable. The `enabled` field will be used to store the current
/// enabling state.
#[derive(Clone, Debug)]
pub(crate) struct TogglingGoal {
    pub(crate) mask: i32,
    pub(crate) physical_key: u64,
    pub(crate) logical_key: u64,
    /// Used by `KeyEmbedderResponder` to store the current enabling state of this modifier.
    ///
    /// Initialized as false.
    pub(crate) enabled: bool,
}

pub(crate) const PRESSING_GOALS: [PressingGoal; 3] = [
    PressingGoal {
        mask: META_CTRL_ON,
        keys: [
            KeyPair {
                physical_key: 0x000700e0,
                logical_key: 0x0200000100,
            }, // ControlLeft
            KeyPair {
                physical_key: 0x000700e4,
                logical_key: 0x0200000101,
            }, // ControlRight
        ],
    },
    PressingGoal {
        mask: META_SHIFT_ON,
        keys: [
            KeyPair {
                physical_key: 0x000700e1,
                logical_key: 0x0200000102,
            }, // ShiftLeft
            KeyPair {
                physical_key: 0x000700e5,
                logical_key: 0x0200000103,
            }, // ShiftRight
        ],
    },
    PressingGoal {
        mask: META_ALT_ON,
        keys: [
            KeyPair {
                physical_key: 0x000700e2,
                logical_key: 0x0200000104,
            }, // AltLeft
            KeyPair {
                physical_key: 0x000700e6,
                logical_key: 0x0200000105,
            }, // AltRight
        ],
    },
];

/// A list of toggling modifiers that must be synchronized on each key event.
///
/// The list is not a static variable but constructed by a function, because `TogglingGoal` is
/// mutable.
pub(crate) fn get_toggling_goals() -> [TogglingGoal; 1] {
    [TogglingGoal {
        mask: META_CAPS_LOCK_ON,
        physical_key: 0x00070039,
        logical_key: 0x0100000104,
        enabled: false,
    }]
}

pub(crate) const VALUE_MASK: u64 = 0x000ffffffff;
pub(crate) const ANDROID_PLANE: u64 = 0x01100000000;

// GENERATED TABLES

/// Flutter's physical key for an Android scan code (`KeyEvent.getScanCode`): `scanCodeToPhysical`.
pub(crate) fn scan_code_to_physical(scan_code: u64) -> Option<u64> {
    let key = match scan_code {
        0x0000000001 => 0x0000070029, // escape
        0x0000000002 => 0x000007001e, // digit1
        0x0000000003 => 0x000007001f, // digit2
        0x0000000004 => 0x0000070020, // digit3
        0x0000000005 => 0x0000070021, // digit4
        0x0000000006 => 0x0000070022, // digit5
        0x0000000007 => 0x0000070023, // digit6
        0x0000000008 => 0x0000070024, // digit7
        0x0000000009 => 0x0000070025, // digit8
        0x000000000a => 0x0000070026, // digit9
        0x000000000b => 0x0000070027, // digit0
        0x000000000c => 0x000007002d, // minus
        0x000000000d => 0x000007002e, // equal
        0x000000000e => 0x000007002a, // backspace
        0x000000000f => 0x000007002b, // tab
        0x0000000010 => 0x0000070014, // keyQ
        0x0000000011 => 0x000007001a, // keyW
        0x0000000012 => 0x0000070008, // keyE
        0x0000000013 => 0x0000070015, // keyR
        0x0000000014 => 0x0000070017, // keyT
        0x0000000015 => 0x000007001c, // keyY
        0x0000000016 => 0x0000070018, // keyU
        0x0000000017 => 0x000007000c, // keyI
        0x0000000018 => 0x0000070012, // keyO
        0x0000000019 => 0x0000070013, // keyP
        0x000000001a => 0x000007002f, // bracketLeft
        0x000000001b => 0x0000070030, // bracketRight
        0x000000001c => 0x0000070028, // enter
        0x000000001d => 0x00000700e0, // controlLeft
        0x000000001e => 0x0000070004, // keyA
        0x000000001f => 0x0000070016, // keyS
        0x0000000020 => 0x0000070007, // keyD
        0x0000000021 => 0x0000070009, // keyF
        0x0000000022 => 0x000007000a, // keyG
        0x0000000023 => 0x000007000b, // keyH
        0x0000000024 => 0x000007000d, // keyJ
        0x0000000025 => 0x000007000e, // keyK
        0x0000000026 => 0x000007000f, // keyL
        0x0000000027 => 0x0000070033, // semicolon
        0x0000000028 => 0x0000070034, // quote
        0x0000000029 => 0x0000070035, // backquote
        0x000000002a => 0x00000700e1, // shiftLeft
        0x000000002b => 0x0000070031, // backslash
        0x000000002c => 0x000007001d, // keyZ
        0x000000002d => 0x000007001b, // keyX
        0x000000002e => 0x0000070006, // keyC
        0x000000002f => 0x0000070019, // keyV
        0x0000000030 => 0x0000070005, // keyB
        0x0000000031 => 0x0000070011, // keyN
        0x0000000032 => 0x0000070010, // keyM
        0x0000000033 => 0x0000070036, // comma
        0x0000000034 => 0x0000070037, // period
        0x0000000035 => 0x0000070038, // slash
        0x0000000036 => 0x00000700e5, // shiftRight
        0x0000000037 => 0x0000070055, // numpadMultiply
        0x0000000038 => 0x00000700e2, // altLeft
        0x0000000039 => 0x000007002c, // space
        0x000000003a => 0x0000070039, // capsLock
        0x000000003b => 0x000007003a, // f1
        0x000000003c => 0x000007003b, // f2
        0x000000003d => 0x000007003c, // f3
        0x000000003e => 0x000007003d, // f4
        0x000000003f => 0x000007003e, // f5
        0x0000000040 => 0x000007003f, // f6
        0x0000000041 => 0x0000070040, // f7
        0x0000000042 => 0x0000070041, // f8
        0x0000000043 => 0x0000070042, // f9
        0x0000000044 => 0x0000070043, // f10
        0x0000000045 => 0x0000070053, // numLock
        0x0000000046 => 0x0000070047, // scrollLock
        0x0000000047 => 0x000007005f, // numpad7
        0x0000000048 => 0x0000070060, // numpad8
        0x0000000049 => 0x0000070061, // numpad9
        0x000000004a => 0x0000070056, // numpadSubtract
        0x000000004b => 0x000007005c, // numpad4
        0x000000004c => 0x000007005d, // numpad5
        0x000000004d => 0x000007005e, // numpad6
        0x000000004e => 0x0000070057, // numpadAdd
        0x000000004f => 0x0000070059, // numpad1
        0x0000000050 => 0x000007005a, // numpad2
        0x0000000051 => 0x000007005b, // numpad3
        0x0000000052 => 0x0000070062, // numpad0
        0x0000000053 => 0x0000070063, // numpadDecimal
        0x0000000056 => 0x0000070031, // backslash
        0x0000000057 => 0x0000070044, // f11
        0x0000000058 => 0x0000070045, // f12
        0x0000000059 => 0x0000070087, // intlRo
        0x000000005a => 0x0000070092, // lang3
        0x000000005b => 0x0000070093, // lang4
        0x000000005c => 0x000007008a, // convert
        0x000000005e => 0x000007008b, // nonConvert
        0x000000005f => 0x0000070085, // numpadComma
        0x0000000060 => 0x0000070058, // numpadEnter
        0x0000000061 => 0x00000700e4, // controlRight
        0x0000000062 => 0x0000070054, // numpadDivide
        0x0000000063 => 0x0000070046, // printScreen
        0x0000000064 => 0x00000700e6, // altRight
        0x0000000066 => 0x000007004a, // home
        0x0000000067 => 0x0000070052, // arrowUp
        0x0000000068 => 0x000007004b, // pageUp
        0x0000000069 => 0x0000070050, // arrowLeft
        0x000000006a => 0x000007004f, // arrowRight
        0x000000006b => 0x000007004d, // end
        0x000000006c => 0x0000070051, // arrowDown
        0x000000006d => 0x000007004e, // pageDown
        0x000000006e => 0x0000070049, // insert
        0x000000006f => 0x000007004c, // delete
        0x0000000071 => 0x000007007f, // audioVolumeMute
        0x0000000072 => 0x0000070081, // audioVolumeDown
        0x0000000073 => 0x0000070080, // audioVolumeUp
        0x0000000074 => 0x0000070066, // power
        0x0000000075 => 0x0000070067, // numpadEqual
        0x0000000077 => 0x0000070048, // pause
        0x0000000079 => 0x0000070085, // numpadComma
        0x000000007c => 0x0000070089, // intlYen
        0x000000007d => 0x00000700e3, // metaLeft
        0x000000007e => 0x00000700e7, // metaRight
        0x000000007f => 0x0000070065, // contextMenu
        0x0000000080 => 0x00000c00b7, // mediaStop
        0x0000000081 => 0x0000070079, // again
        0x0000000082 => 0x00000700a3, // props
        0x0000000083 => 0x000007007a, // undo
        0x0000000085 => 0x000007007c, // copy
        0x0000000086 => 0x0000070074, // open
        0x0000000087 => 0x000007007d, // paste
        0x0000000088 => 0x000007007e, // find
        0x0000000089 => 0x000007007b, // cut
        0x000000008a => 0x0000070075, // help
        0x000000008b => 0x0000070065, // contextMenu
        0x000000008e => 0x0000010082, // sleep
        0x000000008f => 0x0000010083, // wakeUp
        0x0000000098 => 0x0000070066, // power
        0x000000009b => 0x00000c018a, // launchMail
        0x000000009c => 0x00000c022a, // browserFavorites
        0x000000009f => 0x00000c0225, // browserForward
        0x00000000a0 => 0x00000c0203, // close
        0x00000000a1 => 0x00000c00b8, // eject
        0x00000000a2 => 0x00000c00b8, // eject
        0x00000000a3 => 0x00000c00b5, // mediaTrackNext
        0x00000000a4 => 0x00000c00cd, // mediaPlayPause
        0x00000000a5 => 0x00000c00b6, // mediaTrackPrevious
        0x00000000a6 => 0x00000c00b7, // mediaStop
        0x00000000a7 => 0x00000c00b2, // mediaRecord
        0x00000000a8 => 0x00000c00b4, // mediaRewind
        0x00000000ae => 0x00000c0094, // exit
        0x00000000b1 => 0x000007004b, // pageUp
        0x00000000b2 => 0x000007004e, // pageDown
        0x00000000b3 => 0x00000700b6, // numpadParenLeft
        0x00000000b4 => 0x00000700b7, // numpadParenRight
        0x00000000b6 => 0x00000c0279, // redo
        0x00000000b7 => 0x0000070068, // f13
        0x00000000b8 => 0x0000070069, // f14
        0x00000000b9 => 0x000007006a, // f15
        0x00000000ba => 0x000007006b, // f16
        0x00000000bb => 0x000007006c, // f17
        0x00000000bc => 0x000007006d, // f18
        0x00000000bd => 0x000007006e, // f19
        0x00000000be => 0x000007006f, // f20
        0x00000000bf => 0x0000070070, // f21
        0x00000000c0 => 0x0000070071, // f22
        0x00000000c1 => 0x0000070072, // f23
        0x00000000c2 => 0x0000070073, // f24
        0x00000000c8 => 0x00000c00b0, // mediaPlay
        0x00000000c9 => 0x00000c00b1, // mediaPause
        0x00000000cd => 0x0000000014, // suspend
        0x00000000ce => 0x00000c0203, // close
        0x00000000cf => 0x00000c00b0, // mediaPlay
        0x00000000d0 => 0x00000c00b3, // mediaFastForward
        0x00000000d1 => 0x00000c00e5, // bassBoost
        0x00000000d2 => 0x00000c0208, // print
        0x00000000d7 => 0x00000c018a, // launchMail
        0x00000000d9 => 0x00000c0221, // browserSearch
        0x00000000e0 => 0x00000c0070, // brightnessDown
        0x00000000e1 => 0x00000c006f, // brightnessUp
        0x0000000100 => 0x000005ff01, // gameButton1
        0x0000000101 => 0x000005ff02, // gameButton2
        0x0000000102 => 0x000005ff03, // gameButton3
        0x0000000103 => 0x000005ff04, // gameButton4
        0x0000000104 => 0x000005ff05, // gameButton5
        0x0000000105 => 0x000005ff06, // gameButton6
        0x0000000106 => 0x000005ff07, // gameButton7
        0x0000000107 => 0x000005ff08, // gameButton8
        0x0000000108 => 0x000005ff09, // gameButton9
        0x0000000109 => 0x000005ff0a, // gameButton10
        0x000000010a => 0x000005ff0b, // gameButton11
        0x000000010b => 0x000005ff0c, // gameButton12
        0x000000010c => 0x000005ff0d, // gameButton13
        0x000000010d => 0x000005ff0e, // gameButton14
        0x000000010e => 0x000005ff0f, // gameButton15
        0x000000010f => 0x000005ff10, // gameButton16
        0x0000000120 => 0x000005ff01, // gameButton1
        0x0000000121 => 0x000005ff02, // gameButton2
        0x0000000122 => 0x000005ff03, // gameButton3
        0x0000000123 => 0x000005ff04, // gameButton4
        0x0000000124 => 0x000005ff05, // gameButton5
        0x0000000125 => 0x000005ff06, // gameButton6
        0x0000000126 => 0x000005ff07, // gameButton7
        0x0000000127 => 0x000005ff08, // gameButton8
        0x0000000128 => 0x000005ff09, // gameButton9
        0x0000000129 => 0x000005ff0a, // gameButton10
        0x000000012a => 0x000005ff0b, // gameButton11
        0x000000012b => 0x000005ff0c, // gameButton12
        0x000000012c => 0x000005ff0d, // gameButton13
        0x000000012d => 0x000005ff0e, // gameButton14
        0x000000012e => 0x000005ff0f, // gameButton15
        0x000000012f => 0x000005ff10, // gameButton16
        0x0000000130 => 0x000005ff11, // gameButtonA
        0x0000000131 => 0x000005ff12, // gameButtonB
        0x0000000132 => 0x000005ff13, // gameButtonC
        0x0000000133 => 0x000005ff1d, // gameButtonX
        0x0000000134 => 0x000005ff1e, // gameButtonY
        0x0000000135 => 0x000005ff1f, // gameButtonZ
        0x0000000136 => 0x000005ff14, // gameButtonLeft1
        0x0000000137 => 0x000005ff17, // gameButtonRight1
        0x0000000138 => 0x000005ff15, // gameButtonLeft2
        0x0000000139 => 0x000005ff18, // gameButtonRight2
        0x000000013a => 0x000005ff19, // gameButtonSelect
        0x000000013b => 0x000005ff1a, // gameButtonStart
        0x000000013c => 0x000005ff16, // gameButtonMode
        0x000000013d => 0x000005ff1b, // gameButtonThumbLeft
        0x000000013e => 0x000005ff1c, // gameButtonThumbRight
        0x0000000161 => 0x0000070077, // select
        0x0000000166 => 0x00000c0060, // info
        0x0000000172 => 0x00000c0061, // closedCaptionToggle
        0x000000018d => 0x00000c018e, // launchCalendar
        0x0000000192 => 0x00000c009c, // channelUp
        0x0000000193 => 0x00000c009d, // channelDown
        0x0000000195 => 0x00000c0083, // mediaLast
        0x000000019b => 0x0000070048, // pause
        0x00000001ad => 0x00000c018d, // launchContacts
        0x00000001d0 => 0x0000000012, // fn
        0x0000000247 => 0x00000c01cb, // launchAssistant
        _ => return None,
    };
    Some(key)
}

/// Flutter's logical key for an Android key code (`KeyEvent.getKeyCode`): `keyCodeToLogical`.
pub(crate) fn key_code_to_logical(key_code: u64) -> Option<u64> {
    let key = match key_code {
        0x0000000003 => 0x0100001006, // goHome
        0x0000000004 => 0x0100001005, // goBack
        0x0000000005 => 0x0100001002, // call
        0x0000000006 => 0x0100001004, // endCall
        0x0000000007 => 0x0000000030, // digit0
        0x0000000008 => 0x0000000031, // digit1
        0x0000000009 => 0x0000000032, // digit2
        0x000000000a => 0x0000000033, // digit3
        0x000000000b => 0x0000000034, // digit4
        0x000000000c => 0x0000000035, // digit5
        0x000000000d => 0x0000000036, // digit6
        0x000000000e => 0x0000000037, // digit7
        0x000000000f => 0x0000000038, // digit8
        0x0000000010 => 0x0000000039, // digit9
        0x0000000011 => 0x000000002a, // asterisk
        0x0000000012 => 0x0000000023, // numberSign
        0x0000000013 => 0x0100000304, // arrowUp
        0x0000000014 => 0x0100000301, // arrowDown
        0x0000000015 => 0x0100000302, // arrowLeft
        0x0000000016 => 0x0100000303, // arrowRight
        0x0000000017 => 0x010000050c, // select
        0x0000000018 => 0x0100000a10, // audioVolumeUp
        0x0000000019 => 0x0100000a0f, // audioVolumeDown
        0x000000001a => 0x0100000606, // power
        0x000000001b => 0x0100000603, // camera
        0x000000001c => 0x0100000401, // clear
        0x000000001d => 0x0000000061, // keyA
        0x000000001e => 0x0000000062, // keyB
        0x000000001f => 0x0000000063, // keyC
        0x0000000020 => 0x0000000064, // keyD
        0x0000000021 => 0x0000000065, // keyE
        0x0000000022 => 0x0000000066, // keyF
        0x0000000023 => 0x0000000067, // keyG
        0x0000000024 => 0x0000000068, // keyH
        0x0000000025 => 0x0000000069, // keyI
        0x0000000026 => 0x000000006a, // keyJ
        0x0000000027 => 0x000000006b, // keyK
        0x0000000028 => 0x000000006c, // keyL
        0x0000000029 => 0x000000006d, // keyM
        0x000000002a => 0x000000006e, // keyN
        0x000000002b => 0x000000006f, // keyO
        0x000000002c => 0x0000000070, // keyP
        0x000000002d => 0x0000000071, // keyQ
        0x000000002e => 0x0000000072, // keyR
        0x000000002f => 0x0000000073, // keyS
        0x0000000030 => 0x0000000074, // keyT
        0x0000000031 => 0x0000000075, // keyU
        0x0000000032 => 0x0000000076, // keyV
        0x0000000033 => 0x0000000077, // keyW
        0x0000000034 => 0x0000000078, // keyX
        0x0000000035 => 0x0000000079, // keyY
        0x0000000036 => 0x000000007a, // keyZ
        0x0000000037 => 0x000000002c, // comma
        0x0000000038 => 0x000000002e, // period
        0x0000000039 => 0x0200000104, // altLeft
        0x000000003a => 0x0200000105, // altRight
        0x000000003b => 0x0200000102, // shiftLeft
        0x000000003c => 0x0200000103, // shiftRight
        0x000000003d => 0x0100000009, // tab
        0x000000003e => 0x0000000020, // space
        0x000000003f => 0x010000010f, // symbol
        0x0000000040 => 0x0100000b09, // launchWebBrowser
        0x0000000041 => 0x0100000b03, // launchMail
        0x0000000042 => 0x010000000d, // enter
        0x0000000043 => 0x0100000008, // backspace
        0x0000000044 => 0x0000000060, // backquote
        0x0000000045 => 0x000000002d, // minus
        0x0000000046 => 0x000000003d, // equal
        0x0000000047 => 0x000000005b, // bracketLeft
        0x0000000048 => 0x000000005d, // bracketRight
        0x0000000049 => 0x000000005c, // backslash
        0x000000004a => 0x000000003b, // semicolon
        0x000000004b => 0x0000000022, // quote
        0x000000004c => 0x000000002f, // slash
        0x000000004d => 0x0000000040, // at
        0x000000004f => 0x0100001007, // headsetHook
        0x0000000050 => 0x0100001003, // cameraFocus
        0x0000000051 => 0x000000002b, // add
        0x0000000052 => 0x0100000505, // contextMenu
        0x0000000053 => 0x0100001009, // notification
        0x0000000054 => 0x0100000c06, // browserSearch
        0x0000000055 => 0x0100000a05, // mediaPlayPause
        0x0000000056 => 0x0100000a07, // mediaStop
        0x0000000057 => 0x0100000a08, // mediaTrackNext
        0x0000000058 => 0x0100000a09, // mediaTrackPrevious
        0x0000000059 => 0x0100000d31, // mediaRewind
        0x000000005a => 0x0100000d2c, // mediaFastForward
        0x000000005b => 0x0100000e09, // microphoneVolumeMute
        0x000000005c => 0x0100000308, // pageUp
        0x000000005d => 0x0100000307, // pageDown
        0x000000005f => 0x010000070b, // modeChange
        0x0000000060 => 0x0200000311, // gameButtonA
        0x0000000061 => 0x0200000312, // gameButtonB
        0x0000000062 => 0x0200000313, // gameButtonC
        0x0000000063 => 0x020000031d, // gameButtonX
        0x0000000064 => 0x020000031e, // gameButtonY
        0x0000000065 => 0x020000031f, // gameButtonZ
        0x0000000066 => 0x0200000314, // gameButtonLeft1
        0x0000000067 => 0x0200000317, // gameButtonRight1
        0x0000000068 => 0x0200000315, // gameButtonLeft2
        0x0000000069 => 0x0200000318, // gameButtonRight2
        0x000000006a => 0x020000031b, // gameButtonThumbLeft
        0x000000006b => 0x020000031c, // gameButtonThumbRight
        0x000000006c => 0x020000031a, // gameButtonStart
        0x000000006d => 0x0200000319, // gameButtonSelect
        0x000000006e => 0x0200000316, // gameButtonMode
        0x000000006f => 0x010000001b, // escape
        0x0000000070 => 0x010000007f, // delete
        0x0000000071 => 0x0200000100, // controlLeft
        0x0000000072 => 0x0200000101, // controlRight
        0x0000000073 => 0x0100000104, // capsLock
        0x0000000074 => 0x010000010c, // scrollLock
        0x0000000075 => 0x0200000106, // metaLeft
        0x0000000076 => 0x0200000107, // metaRight
        0x0000000077 => 0x0100000106, // fn
        0x0000000078 => 0x0100000608, // printScreen
        0x0000000079 => 0x0100000509, // pause
        0x000000007a => 0x0100000306, // home
        0x000000007b => 0x0100000305, // end
        0x000000007c => 0x0100000407, // insert
        0x000000007d => 0x0100000c03, // browserForward
        0x000000007e => 0x0100000d2f, // mediaPlay
        0x000000007f => 0x0100000d2e, // mediaPause
        0x0000000080 => 0x0100000a01, // close
        0x0000000081 => 0x0100000604, // eject
        0x0000000082 => 0x0100000d30, // mediaRecord
        0x0000000083 => 0x0100000801, // f1
        0x0000000084 => 0x0100000802, // f2
        0x0000000085 => 0x0100000803, // f3
        0x0000000086 => 0x0100000804, // f4
        0x0000000087 => 0x0100000805, // f5
        0x0000000088 => 0x0100000806, // f6
        0x0000000089 => 0x0100000807, // f7
        0x000000008a => 0x0100000808, // f8
        0x000000008b => 0x0100000809, // f9
        0x000000008c => 0x010000080a, // f10
        0x000000008d => 0x010000080b, // f11
        0x000000008e => 0x010000080c, // f12
        0x000000008f => 0x010000010a, // numLock
        0x0000000090 => 0x0200000230, // numpad0
        0x0000000091 => 0x0200000231, // numpad1
        0x0000000092 => 0x0200000232, // numpad2
        0x0000000093 => 0x0200000233, // numpad3
        0x0000000094 => 0x0200000234, // numpad4
        0x0000000095 => 0x0200000235, // numpad5
        0x0000000096 => 0x0200000236, // numpad6
        0x0000000097 => 0x0200000237, // numpad7
        0x0000000098 => 0x0200000238, // numpad8
        0x0000000099 => 0x0200000239, // numpad9
        0x000000009a => 0x020000022f, // numpadDivide
        0x000000009b => 0x020000022a, // numpadMultiply
        0x000000009c => 0x020000022d, // numpadSubtract
        0x000000009d => 0x020000022b, // numpadAdd
        0x000000009e => 0x020000022e, // numpadDecimal
        0x000000009f => 0x020000022c, // numpadComma
        0x00000000a0 => 0x020000020d, // numpadEnter
        0x00000000a1 => 0x020000023d, // numpadEqual
        0x00000000a2 => 0x0200000228, // numpadParenLeft
        0x00000000a3 => 0x0200000229, // numpadParenRight
        0x00000000a4 => 0x0100000a11, // audioVolumeMute
        0x00000000a5 => 0x0100000d25, // info
        0x00000000a6 => 0x0100000d0b, // channelUp
        0x00000000a7 => 0x0100000d0a, // channelDown
        0x00000000a8 => 0x010000050d, // zoomIn
        0x00000000a9 => 0x010000050e, // zoomOut
        0x00000000aa => 0x0100000d49, // tv
        0x00000000ac => 0x0100000d22, // guide
        0x00000000ad => 0x0100000d4f, // dvr
        0x00000000ae => 0x0100000c02, // browserFavorites
        0x00000000af => 0x0100000d12, // closedCaptionToggle
        0x00000000b0 => 0x0100000d43, // settings
        0x00000000b1 => 0x0100000d4b, // tvPower
        0x00000000b2 => 0x0100000d4a, // tvInput
        0x00000000b3 => 0x0100000d46, // stbPower
        0x00000000b4 => 0x0100000d45, // stbInput
        0x00000000b5 => 0x0100000d09, // avrPower
        0x00000000b6 => 0x0100000d08, // avrInput
        0x00000000b7 => 0x0100000d0c, // colorF0Red
        0x00000000b8 => 0x0100000d0d, // colorF1Green
        0x00000000b9 => 0x0100000d0e, // colorF2Yellow
        0x00000000ba => 0x0100000d0f, // colorF3Blue
        0x00000000bb => 0x0100001001, // appSwitch
        0x00000000bc => 0x0200000301, // gameButton1
        0x00000000bd => 0x0200000302, // gameButton2
        0x00000000be => 0x0200000303, // gameButton3
        0x00000000bf => 0x0200000304, // gameButton4
        0x00000000c0 => 0x0200000305, // gameButton5
        0x00000000c1 => 0x0200000306, // gameButton6
        0x00000000c2 => 0x0200000307, // gameButton7
        0x00000000c3 => 0x0200000308, // gameButton8
        0x00000000c4 => 0x0200000309, // gameButton9
        0x00000000c5 => 0x020000030a, // gameButton10
        0x00000000c6 => 0x020000030b, // gameButton11
        0x00000000c7 => 0x020000030c, // gameButton12
        0x00000000c8 => 0x020000030d, // gameButton13
        0x00000000c9 => 0x020000030e, // gameButton14
        0x00000000ca => 0x020000030f, // gameButton15
        0x00000000cb => 0x0200000310, // gameButton16
        0x00000000cc => 0x0100000709, // groupNext
        0x00000000cd => 0x010000100a, // mannerMode
        0x00000000ce => 0x0100001101, // tv3DMode
        0x00000000cf => 0x0100000b0c, // launchContacts
        0x00000000d0 => 0x0100000b02, // launchCalendar
        0x00000000d1 => 0x0100000b05, // launchMusicPlayer
        0x00000000d3 => 0x010000071d, // zenkakuHankaku
        0x00000000d4 => 0x0100000714, // eisu
        0x00000000d5 => 0x010000070d, // nonConvert
        0x00000000d6 => 0x0100000705, // convert
        0x00000000d7 => 0x0100000717, // hiraganaKatakana
        0x00000000d8 => 0x0200000022, // intlYen
        0x00000000d9 => 0x0200000021, // intlRo
        0x00000000da => 0x0100000719, // kanjiMode
        0x00000000db => 0x0100000b0e, // launchAssistant
        0x00000000dc => 0x0100000601, // brightnessDown
        0x00000000dd => 0x0100000602, // brightnessUp
        0x00000000de => 0x0100000d50, // mediaAudioTrack
        0x00000000df => 0x0200000002, // sleep
        0x00000000e0 => 0x010000060b, // wakeUp
        0x00000000e1 => 0x0100000d5a, // pairing
        0x00000000e2 => 0x0100000d55, // mediaTopMenu
        0x00000000e5 => 0x0100000d2d, // mediaLast
        0x00000000e6 => 0x0100001107, // tvDataService
        0x00000000e8 => 0x0100001114, // tvRadioService
        0x00000000e9 => 0x0100000d48, // teletext
        0x00000000ea => 0x0100001113, // tvNumberEntry
        0x00000000eb => 0x0100001119, // tvTerrestrialAnalog
        0x00000000ec => 0x010000111a, // tvTerrestrialDigital
        0x00000000ed => 0x0100001115, // tvSatellite
        0x00000000ee => 0x0100001116, // tvSatelliteBS
        0x00000000ef => 0x0100001117, // tvSatelliteCS
        0x00000000f0 => 0x0100001118, // tvSatelliteToggle
        0x00000000f1 => 0x0100001112, // tvNetwork
        0x00000000f2 => 0x0100001102, // tvAntennaCable
        0x00000000f3 => 0x010000110c, // tvInputHDMI1
        0x00000000f4 => 0x010000110d, // tvInputHDMI2
        0x00000000f5 => 0x010000110e, // tvInputHDMI3
        0x00000000f6 => 0x010000110f, // tvInputHDMI4
        0x00000000f7 => 0x010000110a, // tvInputComposite1
        0x00000000f8 => 0x010000110b, // tvInputComposite2
        0x00000000f9 => 0x0100001108, // tvInputComponent1
        0x00000000fa => 0x0100001109, // tvInputComponent2
        0x00000000fb => 0x0100001110, // tvInputVGA1
        0x00000000fc => 0x0100001103, // tvAudioDescription
        0x00000000fd => 0x0100001105, // tvAudioDescriptionMixUp
        0x00000000fe => 0x0100001104, // tvAudioDescriptionMixDown
        0x00000000ff => 0x0100000d4e, // zoomToggle
        0x0000000100 => 0x0100001106, // tvContentsMenu
        0x0000000102 => 0x010000111b, // tvTimer
        0x0000000103 => 0x0100000508, // help
        0x0000000104 => 0x0100000d59, // navigatePrevious
        0x0000000105 => 0x0100000d57, // navigateNext
        0x0000000106 => 0x0100000d56, // navigateIn
        0x0000000107 => 0x0100000d58, // navigateOut
        0x0000000110 => 0x0100000d52, // mediaSkipForward
        0x0000000111 => 0x0100000d51, // mediaSkipBackward
        0x0000000112 => 0x0100000d54, // mediaStepForward
        0x0000000113 => 0x0100000d53, // mediaStepBackward
        0x0000000115 => 0x0100000404, // cut
        0x0000000116 => 0x0100000402, // copy
        0x0000000117 => 0x0100000408, // paste
        _ => return None,
    };
    Some(key)
}
